// SPDX-License-Identifier: MIT OR Apache-2.0

use abp_core::CONTRACT_VERSION;
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version};

// --- parse_version (legacy free-function) ------------------------------------

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
    assert_eq!(parse_version("abp/v1.2.3"), None);
}

// --- is_compatible_version (legacy free-function) ----------------------------

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

// === ProtocolVersion =========================================================

// --- Parsing -----------------------------------------------------------------

#[test]
fn protocol_version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);

    let v = ProtocolVersion::parse("abp/v12.34").unwrap();
    assert_eq!(v.major, 12);
    assert_eq!(v.minor, 34);
}

#[test]
fn protocol_version_parse_invalid_format() {
    assert_eq!(
        ProtocolVersion::parse("v0.1").unwrap_err(),
        VersionError::InvalidFormat
    );
    assert_eq!(
        ProtocolVersion::parse("").unwrap_err(),
        VersionError::InvalidFormat
    );
    assert_eq!(
        ProtocolVersion::parse("abp/v").unwrap_err(),
        VersionError::InvalidFormat
    );
}

#[test]
fn protocol_version_parse_invalid_major() {
    assert_eq!(
        ProtocolVersion::parse("abp/vx.1").unwrap_err(),
        VersionError::InvalidMajor
    );
}

#[test]
fn protocol_version_parse_invalid_minor() {
    assert_eq!(
        ProtocolVersion::parse("abp/v1.y").unwrap_err(),
        VersionError::InvalidMinor
    );
    assert_eq!(
        ProtocolVersion::parse("abp/v1.2.3").unwrap_err(),
        VersionError::InvalidMinor
    );
}

// --- Compatibility -----------------------------------------------------------

#[test]
fn protocol_version_compatible_same_major_higher_minor() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    assert!(v01.is_compatible(&v02));
}

#[test]
fn protocol_version_compatible_exact() {
    let v = ProtocolVersion { major: 1, minor: 3 };
    assert!(v.is_compatible(&v));
}

#[test]
fn protocol_version_not_compatible_lower_minor() {
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    assert!(!v02.is_compatible(&v01));
}

#[test]
fn protocol_version_incompatible_different_major() {
    let v0 = ProtocolVersion { major: 0, minor: 5 };
    let v1 = ProtocolVersion { major: 1, minor: 0 };
    assert!(!v0.is_compatible(&v1));
    assert!(!v1.is_compatible(&v0));
}

// --- Ordering ----------------------------------------------------------------

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01 < v02);
    assert!(v02 < v10);
    assert!(v01 < v10);
}

// --- current() ---------------------------------------------------------------

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    let expected = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert_eq!(current, expected);
}

// --- Display / to_string -----------------------------------------------------

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(format!("{v}"), "abp/v0.1");
    assert_eq!(v.to_string(), "abp/v0.1");
}

// --- Serde roundtrip ---------------------------------------------------------

#[test]
fn protocol_version_serde_roundtrip() {
    let v = ProtocolVersion { major: 2, minor: 7 };
    let json = serde_json::to_string(&v).unwrap();
    let deserialized: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, deserialized);
}

// === VersionRange ============================================================

#[test]
fn version_range_contains() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 5 }));
}

#[test]
fn version_range_out_of_bounds() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 2 },
        max: ProtocolVersion { major: 0, minor: 4 },
    };
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 5 }));
    assert!(!range.contains(&ProtocolVersion { major: 1, minor: 3 }));
}

#[test]
fn version_range_is_compatible() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

// === negotiate_version =======================================================

#[test]
fn negotiate_version_success_picks_min() {
    let local = ProtocolVersion { major: 0, minor: 3 };
    let remote = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&local, &remote).unwrap();
    assert_eq!(result, ProtocolVersion { major: 0, minor: 1 });
}

#[test]
fn negotiate_version_same() {
    let v = ProtocolVersion { major: 1, minor: 2 };
    assert_eq!(negotiate_version(&v, &v).unwrap(), v);
}

#[test]
fn negotiate_version_failure_different_major() {
    let local = ProtocolVersion { major: 0, minor: 1 };
    let remote = ProtocolVersion { major: 1, minor: 0 };
    let err = negotiate_version(&local, &remote).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}
