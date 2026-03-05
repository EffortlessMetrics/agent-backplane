#![allow(clippy::all)]
#![allow(dead_code, unused_imports)]
#![allow(clippy::manual_repeat_n)]
#![allow(clippy::manual_range_contains)]
#![allow(clippy::single_component_path_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unnecessary_to_owned)]
#![allow(clippy::implicit_clone)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::iter_kv_map)]
#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::redundant_closure)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_match)]
#![allow(clippy::single_match)]
#![allow(clippy::manual_map)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::needless_return)]
#![allow(clippy::redundant_pattern_matching)]
#![allow(clippy::len_zero)]
#![allow(clippy::map_entry)]
#![allow(clippy::unnecessary_unwrap)]
#![allow(unknown_lints)]
#![allow(clippy::needless_borrow)]
#![allow(clippy::type_complexity)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::useless_vec)]
#![allow(clippy::needless_update)]
#![allow(clippy::approx_constant)]
// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol version negotiation and backward/forward compatibility tests.

use abp_core::{
    BackendIdentity, CapabilityManifest, ExecutionMode, Receipt, ReceiptBuilder, CONTRACT_VERSION,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec};

// =========================================================================
// 1. Current contract version is "abp/v0.1"
// =========================================================================

#[test]
fn contract_version_is_abp_v0_1() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
    assert_eq!(current.minor, 1);
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn contract_version_starts_with_abp_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
}

// =========================================================================
// 2. Version string format validation
// =========================================================================

#[test]
fn valid_version_format_accepted() {
    assert!(parse_version("abp/v0.1").is_some());
    assert!(parse_version("abp/v1.0").is_some());
    assert!(parse_version("abp/v99.99").is_some());
}

#[test]
fn missing_prefix_rejected() {
    assert!(parse_version("v0.1").is_none());
    assert!(parse_version("0.1").is_none());
    assert!(parse_version("apb/v0.1").is_none());
}

#[test]
fn missing_dot_separator_rejected() {
    assert!(parse_version("abp/v01").is_none());
    assert!(parse_version("abp/v1").is_none());
}

#[test]
fn empty_string_rejected() {
    assert!(parse_version("").is_none());
}

#[test]
fn garbage_input_rejected() {
    assert!(parse_version("garbage").is_none());
    assert!(parse_version("abp/vX.Y").is_none());
    assert!(parse_version("abp/v-1.0").is_none());
}

#[test]
fn extra_segments_rejected() {
    // "abp/v1.2.3" has extra text after minor that won't parse as u32
    assert!(parse_version("abp/v1.2.3").is_none());
}

#[test]
fn protocol_version_parse_invalid_format() {
    assert_eq!(
        ProtocolVersion::parse("invalid"),
        Err(VersionError::InvalidFormat)
    );
}

#[test]
fn protocol_version_parse_missing_dot() {
    assert_eq!(
        ProtocolVersion::parse("abp/v1"),
        Err(VersionError::InvalidFormat)
    );
}

#[test]
fn protocol_version_parse_bad_major() {
    assert_eq!(
        ProtocolVersion::parse("abp/vX.1"),
        Err(VersionError::InvalidMajor)
    );
}

#[test]
fn protocol_version_parse_bad_minor() {
    assert_eq!(
        ProtocolVersion::parse("abp/v1.Y"),
        Err(VersionError::InvalidMinor)
    );
}

// =========================================================================
// 3. Version parsing (major.minor)
// =========================================================================

#[test]
fn parse_version_extracts_major_minor() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
    assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
    assert_eq!(parse_version("abp/v10.20"), Some((10, 20)));
}

#[test]
fn protocol_version_parse_extracts_fields() {
    let v = ProtocolVersion::parse("abp/v3.7").unwrap();
    assert_eq!(v.major, 3);
    assert_eq!(v.minor, 7);
}

#[test]
fn parse_version_zero_zero() {
    assert_eq!(parse_version("abp/v0.0"), Some((0, 0)));
}

#[test]
fn protocol_version_parse_large_numbers() {
    let v = ProtocolVersion::parse("abp/v999.888").unwrap();
    assert_eq!(v.major, 999);
    assert_eq!(v.minor, 888);
}

// =========================================================================
// 4. Compatible version matching
// =========================================================================

#[test]
fn same_version_is_compatible() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
}

#[test]
fn same_major_different_minor_compatible() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
    assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
}

#[test]
fn protocol_version_is_compatible_newer_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    // v0.1 is_compatible with v0.2 means other.minor >= self.minor
    assert!(v01.is_compatible(&v02));
}

#[test]
fn protocol_version_is_compatible_same() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert!(v.is_compatible(&v));
}

#[test]
fn protocol_version_not_compatible_older_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    // v0.2 requires at least minor 2, so v0.1 is NOT compatible
    assert!(!v02.is_compatible(&v01));
}

// =========================================================================
// 5. Incompatible version rejection
// =========================================================================

#[test]
fn different_major_is_incompatible() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
}

#[test]
fn invalid_version_strings_incompatible() {
    assert!(!is_compatible_version("garbage", "abp/v0.1"));
    assert!(!is_compatible_version("abp/v0.1", "garbage"));
    assert!(!is_compatible_version("garbage", "nope"));
}

#[test]
fn protocol_version_incompatible_different_major() {
    let v0 = ProtocolVersion::parse("abp/v0.5").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(!v0.is_compatible(&v1));
    assert!(!v1.is_compatible(&v0));
}

// =========================================================================
// 6. Version negotiation between host and sidecar
// =========================================================================

#[test]
fn negotiate_same_version_returns_it() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result, v);
}

#[test]
fn negotiate_compatible_returns_minimum() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result, v01); // min of the two
}

#[test]
fn negotiate_compatible_symmetric() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let r1 = negotiate_version(&v01, &v02).unwrap();
    let r2 = negotiate_version(&v02, &v01).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn negotiate_incompatible_returns_error() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&v0, &v1).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

#[test]
fn negotiate_incompatible_error_contains_versions() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    match negotiate_version(&local, &remote) {
        Err(VersionError::Incompatible {
            local: l,
            remote: r,
        }) => {
            assert_eq!(l, local);
            assert_eq!(r, remote);
        }
        other => panic!("expected Incompatible, got {:?}", other),
    }
}

// =========================================================================
// 7. Forward compatibility (newer sidecar, older host)
// =========================================================================

#[test]
fn newer_minor_sidecar_compatible_with_older_host() {
    // Host speaks v0.1, sidecar speaks v0.3 — same major, compatible
    assert!(is_compatible_version("abp/v0.3", "abp/v0.1"));
}

#[test]
fn negotiate_newer_sidecar_older_host() {
    let host = ProtocolVersion::parse("abp/v0.1").unwrap();
    let sidecar = ProtocolVersion::parse("abp/v0.5").unwrap();
    let effective = negotiate_version(&host, &sidecar).unwrap();
    // Should pick the minimum minor
    assert_eq!(effective.minor, 1);
    assert_eq!(effective.major, 0);
}

#[test]
fn newer_major_sidecar_incompatible_with_older_host() {
    let host = ProtocolVersion::parse("abp/v0.1").unwrap();
    let sidecar = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(negotiate_version(&host, &sidecar).is_err());
}

// =========================================================================
// 8. Backward compatibility (older sidecar, newer host)
// =========================================================================

#[test]
fn older_minor_sidecar_compatible_with_newer_host() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.5"));
}

#[test]
fn negotiate_older_sidecar_newer_host() {
    let host = ProtocolVersion::parse("abp/v0.5").unwrap();
    let sidecar = ProtocolVersion::parse("abp/v0.1").unwrap();
    let effective = negotiate_version(&host, &sidecar).unwrap();
    assert_eq!(effective.minor, 1);
}

#[test]
fn older_major_sidecar_incompatible_with_newer_host() {
    let host = ProtocolVersion::parse("abp/v2.0").unwrap();
    let sidecar = ProtocolVersion::parse("abp/v1.5").unwrap();
    assert!(negotiate_version(&host, &sidecar).is_err());
}

// =========================================================================
// 9. Version in hello envelope
// =========================================================================

#[test]
fn hello_envelope_contains_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello envelope"),
    }
}

#[test]
fn hello_envelope_serialization_includes_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    assert!(json.contains(&format!("\"contract_version\":\"{}\"", CONTRACT_VERSION)));
}

#[test]
fn hello_envelope_deserialization_preserves_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-backend".into(),
            backend_version: Some("1.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_custom_version_round_trips() {
    // Build a hello, re-serialize with a different version, and verify round-trip
    let hello = Envelope::Hello {
        contract_version: "abp/v0.2".to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "abp/v0.2");
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn hello_with_mode_uses_contract_version() {
    let hello = Envelope::hello_with_mode(
        BackendIdentity {
            id: "sidecar".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
        ExecutionMode::Passthrough,
    );
    match hello {
        Envelope::Hello {
            contract_version,
            mode,
            ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            assert_eq!(mode, ExecutionMode::Passthrough);
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 10. Version in receipt metadata
// =========================================================================

#[test]
fn receipt_builder_uses_contract_version() {
    let receipt = ReceiptBuilder::new("mock").build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_contract_version_serializes() {
    let receipt = ReceiptBuilder::new("mock").build();
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains(&format!("\"contract_version\":\"{}\"", CONTRACT_VERSION)));
}

#[test]
fn receipt_contract_version_round_trips() {
    let receipt = ReceiptBuilder::new("mock").build();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_in_final_envelope_preserves_version() {
    let receipt = ReceiptBuilder::new("mock").build();
    let envelope = Envelope::Final {
        ref_id: "run-1".into(),
        receipt: receipt.clone(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Final { receipt: r, .. } => {
            assert_eq!(r.meta.contract_version, CONTRACT_VERSION);
        }
        _ => panic!("expected Final"),
    }
}

// =========================================================================
// 11. Version mismatch error messages
// =========================================================================

#[test]
fn incompatible_error_displays_both_versions() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&local, &remote).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("abp/v0.1"),
        "message should contain local version: {msg}"
    );
    assert!(
        msg.contains("abp/v1.0"),
        "message should contain remote version: {msg}"
    );
}

#[test]
fn incompatible_error_mentions_incompatible() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v2.0").unwrap();
    let err = negotiate_version(&local, &remote).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("incompatible"),
        "message should mention incompatible: {msg}"
    );
}

#[test]
fn invalid_format_error_display() {
    let err = ProtocolVersion::parse("bad").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid"), "should mention invalid: {msg}");
}

#[test]
fn invalid_major_error_display() {
    let err = ProtocolVersion::parse("abp/vX.1").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("major"), "should mention major: {msg}");
}

#[test]
fn invalid_minor_error_display() {
    let err = ProtocolVersion::parse("abp/v1.Z").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("minor"), "should mention minor: {msg}");
}

// =========================================================================
// 12. Multiple version ranges
// =========================================================================

#[test]
fn version_range_contains_min() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
}

#[test]
fn version_range_contains_max() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 5 }));
}

#[test]
fn version_range_contains_middle() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
}

#[test]
fn version_range_excludes_below_min() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 2 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 1 }));
}

#[test]
fn version_range_excludes_above_max() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 6 }));
}

#[test]
fn version_range_is_compatible_same_major() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
}

#[test]
fn version_range_not_compatible_different_major() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 3 }));
}

#[test]
fn version_range_not_compatible_out_of_range() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 2 },
        max: ProtocolVersion { major: 0, minor: 4 },
    };
    assert!(!range.is_compatible(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(!range.is_compatible(&ProtocolVersion { major: 0, minor: 5 }));
}

#[test]
fn multiple_ranges_check() {
    let ranges = [
        VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        },
        VersionRange {
            min: ProtocolVersion { major: 1, minor: 0 },
            max: ProtocolVersion { major: 1, minor: 2 },
        },
    ];

    let v_0_2 = ProtocolVersion { major: 0, minor: 2 };
    let v_1_1 = ProtocolVersion { major: 1, minor: 1 };
    let v_2_0 = ProtocolVersion { major: 2, minor: 0 };

    assert!(ranges.iter().any(|r| r.is_compatible(&v_0_2)));
    assert!(ranges.iter().any(|r| r.is_compatible(&v_1_1)));
    assert!(!ranges.iter().any(|r| r.is_compatible(&v_2_0)));
}

#[test]
fn single_version_range() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 1 },
    };
    assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 2 }));
}

// =========================================================================
// 13. Semver-like comparison logic
// =========================================================================

#[test]
fn protocol_version_ord_by_major_then_minor() {
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    let v10 = ProtocolVersion { major: 1, minor: 0 };
    assert!(v01 < v02);
    assert!(v02 < v10);
    assert!(v01 < v10);
}

#[test]
fn protocol_version_eq() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(a, b);
}

#[test]
fn protocol_version_ne_different_minor() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 0, minor: 2 };
    assert_ne!(a, b);
}

#[test]
fn protocol_version_ne_different_major() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 1, minor: 1 };
    assert_ne!(a, b);
}

#[test]
fn negotiate_picks_lower_minor_in_same_major() {
    let v03 = ProtocolVersion::parse("abp/v0.3").unwrap();
    let v07 = ProtocolVersion::parse("abp/v0.7").unwrap();
    let result = negotiate_version(&v03, &v07).unwrap();
    assert_eq!(result.minor, 3);
}

#[test]
fn negotiate_picks_lower_across_high_minors() {
    let v100 = ProtocolVersion::parse("abp/v0.100").unwrap();
    let v200 = ProtocolVersion::parse("abp/v0.200").unwrap();
    let result = negotiate_version(&v100, &v200).unwrap();
    assert_eq!(result.minor, 100);
}

#[test]
fn version_sorting() {
    let mut versions = [
        ProtocolVersion { major: 1, minor: 0 },
        ProtocolVersion { major: 0, minor: 2 },
        ProtocolVersion { major: 0, minor: 1 },
        ProtocolVersion { major: 1, minor: 1 },
    ];
    versions.sort();
    assert_eq!(versions[0], ProtocolVersion { major: 0, minor: 1 });
    assert_eq!(versions[1], ProtocolVersion { major: 0, minor: 2 });
    assert_eq!(versions[2], ProtocolVersion { major: 1, minor: 0 });
    assert_eq!(versions[3], ProtocolVersion { major: 1, minor: 1 });
}

// =========================================================================
// 14. Version serialization/deserialization
// =========================================================================

#[test]
fn protocol_version_to_string_format() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(v.to_string(), "abp/v0.1");
}

#[test]
fn protocol_version_display_format() {
    let v = ProtocolVersion { major: 2, minor: 3 };
    assert_eq!(format!("{v}"), "abp/v2.3");
}

#[test]
fn protocol_version_serde_round_trip() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let json = serde_json::to_string(&v).unwrap();
    let deserialized: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, deserialized);
}

#[test]
fn version_range_serde_round_trip() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    let json = serde_json::to_string(&range).unwrap();
    let deserialized: VersionRange = serde_json::from_str(&json).unwrap();
    assert_eq!(range, deserialized);
}

#[test]
fn protocol_version_json_has_major_minor() {
    let v = ProtocolVersion { major: 3, minor: 7 };
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"major\":3"));
    assert!(json.contains("\"minor\":7"));
}

#[test]
fn version_error_serde_variants() {
    // VersionError derives Clone + PartialEq, verify basic equality
    let e1 = VersionError::InvalidFormat;
    let e2 = VersionError::InvalidFormat;
    assert_eq!(e1, e2);

    let e3 = VersionError::InvalidMajor;
    assert_ne!(e1, e3);
}

#[test]
fn protocol_version_parse_and_to_string_round_trip() {
    for version_str in &["abp/v0.0", "abp/v0.1", "abp/v1.0", "abp/v99.99"] {
        let parsed = ProtocolVersion::parse(version_str).unwrap();
        assert_eq!(&parsed.to_string(), version_str);
    }
}

#[test]
fn protocol_version_hash_consistent() {
    use std::collections::HashSet;
    let v1 = ProtocolVersion { major: 0, minor: 1 };
    let v2 = ProtocolVersion { major: 0, minor: 1 };
    let mut set = HashSet::new();
    set.insert(v1);
    assert!(set.contains(&v2));
}

// =========================================================================
// 15. Contract stability guarantees
// =========================================================================

#[test]
fn contract_version_is_parseable() {
    let parsed = parse_version(CONTRACT_VERSION);
    assert!(parsed.is_some(), "CONTRACT_VERSION must be parseable");
}

#[test]
fn contract_version_protocol_version_parse() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION);
    assert!(v.is_ok(), "CONTRACT_VERSION must parse as ProtocolVersion");
}

#[test]
fn current_version_compatible_with_itself() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

#[test]
fn current_protocol_version_compatible_with_itself() {
    let current = ProtocolVersion::current();
    assert!(current.is_compatible(&current));
}

#[test]
fn contract_version_in_v0_range() {
    let (major, _minor) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0, "v0.1 should be in the v0 range");
}

#[test]
fn receipt_always_gets_current_contract_version() {
    // ReceiptBuilder hardcodes CONTRACT_VERSION
    let r1 = ReceiptBuilder::new("backend-a").build();
    let r2 = ReceiptBuilder::new("backend-b").build();
    assert_eq!(r1.meta.contract_version, r2.meta.contract_version);
    assert_eq!(r1.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn hello_always_gets_current_contract_version() {
    let h1 = Envelope::hello(
        BackendIdentity {
            id: "a".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let h2 = Envelope::hello(
        BackendIdentity {
            id: "b".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let extract = |e: &Envelope| match e {
        Envelope::Hello {
            contract_version, ..
        } => contract_version.clone(),
        _ => panic!("not hello"),
    };
    assert_eq!(extract(&h1), extract(&h2));
    assert_eq!(extract(&h1), CONTRACT_VERSION);
}

// =========================================================================
// Additional edge-case and integration tests
// =========================================================================

#[test]
fn negotiate_version_v0_0_with_v0_1() {
    let v00 = ProtocolVersion::parse("abp/v0.0").unwrap();
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v00, &v01).unwrap();
    assert_eq!(result, v00); // min is v0.0
}

#[test]
fn version_range_cross_major_not_compatible() {
    // Range spanning major versions: both bounds must match version's major
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 5 },
        max: ProtocolVersion { major: 1, minor: 3 },
    };
    // Major 0 version: min.major == 0 matches, but max.major == 1 != 0, so not compatible
    assert!(!range.is_compatible(&ProtocolVersion { major: 0, minor: 7 }));
    // Major 1 version: min.major == 0 != 1, so not compatible either
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 1 }));
}

#[test]
fn protocol_version_clone_eq() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let cloned = v.clone();
    assert_eq!(v, cloned);
}

#[test]
fn version_error_clone_eq() {
    let err = VersionError::Incompatible {
        local: ProtocolVersion { major: 0, minor: 1 },
        remote: ProtocolVersion { major: 1, minor: 0 },
    };
    let cloned = err.clone();
    assert_eq!(err, cloned);
}

#[test]
fn hello_envelope_with_future_version_deserializes() {
    // Simulate a sidecar sending a future version by constructing and mutating
    let hello = Envelope::Hello {
        contract_version: "abp/v99.99".to_string(),
        backend: BackendIdentity {
            id: "future".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let json = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, "abp/v99.99");
            // Host should be able to check compatibility
            assert!(!is_compatible_version(&contract_version, CONTRACT_VERSION));
        }
        _ => panic!("expected Hello"),
    }
}

#[test]
fn version_parse_preserves_zero_padded_equivalence() {
    // "abp/v00.01" — leading zeros are valid in u32 parse
    let v = parse_version("abp/v00.01");
    assert_eq!(v, Some((0, 1)));
}

#[test]
fn version_range_serialized_fields() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    let json = serde_json::to_value(&range).unwrap();
    assert!(json.get("min").is_some());
    assert!(json.get("max").is_some());
}

#[test]
fn negotiate_version_reflexive() {
    // For any valid version, negotiating with itself should succeed
    for ver_str in &["abp/v0.0", "abp/v0.1", "abp/v1.0", "abp/v5.10"] {
        let v = ProtocolVersion::parse(ver_str).unwrap();
        let result = negotiate_version(&v, &v).unwrap();
        assert_eq!(result, v);
    }
}

#[test]
fn negotiate_version_high_minor_spread() {
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v99 = ProtocolVersion::parse("abp/v0.99").unwrap();
    let result = negotiate_version(&v1, &v99).unwrap();
    assert_eq!(result.minor, 1);
}

// =========================================================================
// 16. Envelope validation with version fields
// =========================================================================

#[test]
fn validator_accepts_valid_hello_version() {
    use abp_protocol::validate::EnvelopeValidator;
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );
    let result = EnvelopeValidator::new().validate(&hello);
    assert!(result.valid);
    assert!(result.errors.is_empty());
}

#[test]
fn validator_rejects_empty_contract_version() {
    use abp_protocol::validate::EnvelopeValidator;
    let hello = Envelope::Hello {
        contract_version: String::new(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&hello);
    assert!(!result.valid);
    assert!(result
        .errors
        .iter()
        .any(|e| matches!(e, abp_protocol::validate::ValidationError::EmptyField { field } if field == "contract_version")));
}

#[test]
fn validator_rejects_unparseable_contract_version() {
    use abp_protocol::validate::EnvelopeValidator;
    let hello = Envelope::Hello {
        contract_version: "not-a-version".into(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let result = EnvelopeValidator::new().validate(&hello);
    assert!(!result.valid);
    assert!(result.errors.iter().any(|e| matches!(
        e,
        abp_protocol::validate::ValidationError::InvalidVersion { .. }
    )));
}

// =========================================================================
// 17. Version mismatch error code integration
// =========================================================================

#[test]
fn fatal_envelope_with_version_mismatch_code() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "version mismatch",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    assert_eq!(
        fatal.error_code(),
        Some(abp_error::ErrorCode::ProtocolVersionMismatch)
    );
}

#[test]
fn fatal_version_mismatch_round_trips_through_jsonl() {
    let fatal = Envelope::fatal_with_code(
        Some("run-1".into()),
        "incompatible protocol version",
        abp_error::ErrorCode::ProtocolVersionMismatch,
    );
    let json = JsonlCodec::encode(&fatal).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(
        decoded.error_code(),
        Some(abp_error::ErrorCode::ProtocolVersionMismatch)
    );
}

#[test]
fn version_mismatch_error_code_category_is_protocol() {
    assert_eq!(
        abp_error::ErrorCode::ProtocolVersionMismatch.category(),
        abp_error::ErrorCategory::Protocol
    );
}

// =========================================================================
// 18. Work order version context
// =========================================================================

#[test]
fn work_order_in_run_envelope_round_trips() {
    use abp_core::WorkOrderBuilder;
    let wo = WorkOrderBuilder::new("test task").build();
    let envelope = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo.clone(),
    };
    let json = JsonlCodec::encode(&envelope).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    match decoded {
        Envelope::Run { work_order, .. } => {
            assert_eq!(work_order.task, "test task");
        }
        _ => panic!("expected Run"),
    }
}

// =========================================================================
// 19. Multi-version protocol sequence scenarios
// =========================================================================

#[test]
fn sequence_hello_with_current_version_then_run_validates() {
    use abp_core::WorkOrderBuilder;
    use abp_protocol::validate::EnvelopeValidator;
    let hello = Envelope::hello(
        BackendIdentity {
            id: "sidecar".into(),
            backend_version: Some("1.0".into()),
            adapter_version: Some("0.1".into()),
        },
        CapabilityManifest::new(),
    );
    let wo = WorkOrderBuilder::new("do stuff").build();
    let run = Envelope::Run {
        id: "run-1".into(),
        work_order: wo,
    };
    let receipt = ReceiptBuilder::new("sidecar").build();
    let fin = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let errors = EnvelopeValidator::new().validate_sequence(&[hello, run, fin]);
    assert!(errors.is_empty(), "sequence should be valid: {errors:?}");
}

#[test]
fn negotiate_then_build_hello_with_effective_version() {
    let host = ProtocolVersion::parse("abp/v0.3").unwrap();
    let sidecar = ProtocolVersion::parse("abp/v0.1").unwrap();
    let effective = negotiate_version(&host, &sidecar).unwrap();
    let hello = Envelope::Hello {
        contract_version: effective.to_string(),
        backend: BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    match &hello {
        Envelope::Hello {
            contract_version, ..
        } => assert_eq!(contract_version, "abp/v0.1"),
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 20. Version display and debug formatting
// =========================================================================

#[test]
fn protocol_version_debug_includes_fields() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let dbg = format!("{v:?}");
    assert!(dbg.contains("major"));
    assert!(dbg.contains("minor"));
}

#[test]
fn version_error_debug_format() {
    let err = VersionError::Incompatible {
        local: ProtocolVersion { major: 0, minor: 1 },
        remote: ProtocolVersion { major: 1, minor: 0 },
    };
    let dbg = format!("{err:?}");
    assert!(dbg.contains("Incompatible"));
}

#[test]
fn version_range_debug_format() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    let dbg = format!("{range:?}");
    assert!(dbg.contains("VersionRange"));
}

// =========================================================================
// 21. JSON raw manipulation with version fields
// =========================================================================

#[test]
fn hello_json_raw_version_field_exists() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = JsonlCodec::encode(&hello).unwrap();
    let raw: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    assert_eq!(raw["contract_version"], CONTRACT_VERSION);
    assert_eq!(raw["t"], "hello");
}

#[test]
fn receipt_json_raw_contract_version_field() {
    let receipt = ReceiptBuilder::new("mock").build();
    let json = serde_json::to_value(&receipt).unwrap();
    assert_eq!(json["meta"]["contract_version"], CONTRACT_VERSION);
}

#[test]
fn decode_hello_with_unknown_extra_fields_still_works() {
    // Forward compatibility: extra fields are ignored by serde(deny_unknown_fields is not set)
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped","future_field":"ignored"}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    assert!(matches!(decoded, Envelope::Hello { .. }));
}

#[test]
fn decode_hello_with_missing_mode_uses_default() {
    // `mode` has #[serde(default)], so missing is OK
    let raw = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let decoded = JsonlCodec::decode(raw).unwrap();
    match decoded {
        Envelope::Hello { mode, .. } => {
            assert_eq!(mode, ExecutionMode::default());
        }
        _ => panic!("expected Hello"),
    }
}

// =========================================================================
// 22. Version string edge cases
// =========================================================================

#[test]
fn parse_version_with_whitespace_rejected() {
    assert!(parse_version(" abp/v0.1").is_none());
    assert!(parse_version("abp/v0.1 ").is_none());
    assert!(parse_version("abp/ v0.1").is_none());
}

#[test]
fn parse_version_case_sensitive() {
    assert!(parse_version("ABP/v0.1").is_none());
    assert!(parse_version("Abp/v0.1").is_none());
    assert!(parse_version("abp/V0.1").is_none());
}

#[test]
fn parse_version_max_u32_boundary() {
    let max = u32::MAX;
    let ver = format!("abp/v{max}.0");
    assert_eq!(parse_version(&ver), Some((max, 0)));
}

#[test]
fn parse_version_overflow_rejected() {
    // u32::MAX + 1 overflows
    let too_big = format!("abp/v{}.0", u64::from(u32::MAX) + 1);
    assert!(parse_version(&too_big).is_none());
}

#[test]
fn protocol_version_parse_negative_rejected() {
    assert_eq!(
        ProtocolVersion::parse("abp/v-1.0"),
        Err(VersionError::InvalidMajor)
    );
    assert_eq!(
        ProtocolVersion::parse("abp/v0.-1"),
        Err(VersionError::InvalidMinor)
    );
}

// =========================================================================
// 23. Receipt hashing stability across versions
// =========================================================================

#[test]
fn receipt_hash_deterministic_with_version() {
    let r1 = ReceiptBuilder::new("mock").build();
    let r2 = ReceiptBuilder::new("mock").build();
    // Both use CONTRACT_VERSION and same backend
    assert_eq!(r1.meta.contract_version, r2.meta.contract_version);
}

#[test]
fn receipt_with_hash_preserves_contract_version() {
    let receipt = ReceiptBuilder::new("mock").build().with_hash().unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    assert!(receipt.receipt_sha256.is_some());
}

// =========================================================================
// 24. Version negotiation transitivity
// =========================================================================

#[test]
fn negotiate_transitive_within_same_major() {
    let a = ProtocolVersion::parse("abp/v0.1").unwrap();
    let b = ProtocolVersion::parse("abp/v0.3").unwrap();
    let c = ProtocolVersion::parse("abp/v0.5").unwrap();
    let ab = negotiate_version(&a, &b).unwrap();
    let bc = negotiate_version(&b, &c).unwrap();
    let ac = negotiate_version(&a, &c).unwrap();
    // min(a,b) should equal a, min(b,c) should equal b, min(a,c) should equal a
    assert_eq!(ab, a);
    assert_eq!(bc, b);
    assert_eq!(ac, a);
}

#[test]
fn negotiate_commutative() {
    let a = ProtocolVersion::parse("abp/v0.3").unwrap();
    let b = ProtocolVersion::parse("abp/v0.7").unwrap();
    assert_eq!(
        negotiate_version(&a, &b).unwrap(),
        negotiate_version(&b, &a).unwrap()
    );
}

// =========================================================================
// 25. Envelope stream with version checking
// =========================================================================

#[test]
fn decode_stream_multiple_hello_envelopes_with_versions() {
    use std::io::BufReader;
    let h1 = Envelope::Hello {
        contract_version: "abp/v0.1".into(),
        backend: BackendIdentity {
            id: "a".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let h2 = Envelope::Hello {
        contract_version: "abp/v0.2".into(),
        backend: BackendIdentity {
            id: "b".into(),
            backend_version: None,
            adapter_version: None,
        },
        capabilities: CapabilityManifest::new(),
        mode: ExecutionMode::default(),
    };
    let mut buf = Vec::new();
    JsonlCodec::encode_many_to_writer(&mut buf, &[h1, h2]).unwrap();
    let reader = BufReader::new(buf.as_slice());
    let envelopes: Vec<_> = JsonlCodec::decode_stream(reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(envelopes.len(), 2);
    // Extract versions
    let versions: Vec<String> = envelopes
        .iter()
        .filter_map(|e| match e {
            Envelope::Hello {
                contract_version, ..
            } => Some(contract_version.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(versions, vec!["abp/v0.1", "abp/v0.2"]);
}

#[test]
fn encode_to_writer_preserves_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "w".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let mut buf = Vec::new();
    JsonlCodec::encode_to_writer(&mut buf, &hello).unwrap();
    let line = String::from_utf8(buf).unwrap();
    assert!(line.contains(CONTRACT_VERSION));
}
