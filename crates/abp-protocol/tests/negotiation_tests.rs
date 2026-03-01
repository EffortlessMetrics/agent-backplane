// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive protocol version negotiation tests.
//!
//! Covers parsing, ordering, negotiation semantics, edge cases, serde
//! roundtrips, envelope integration, and trait-level guarantees for
//! [`ProtocolVersion`], [`VersionRange`], and [`negotiate_version`].

use std::collections::BTreeMap;

use abp_core::{BackendIdentity, CONTRACT_VERSION, CapabilityManifest};
use abp_protocol::version::{ProtocolVersion, VersionError, VersionRange, negotiate_version};
use abp_protocol::{Envelope, JsonlCodec};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Version parsing
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_v0_1() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!((v.major, v.minor), (0, 1));
}

#[test]
fn parse_v1_0() {
    let v = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert_eq!((v.major, v.minor), (1, 0));
}

#[test]
fn parse_v2_5() {
    let v = ProtocolVersion::parse("abp/v2.5").unwrap();
    assert_eq!((v.major, v.minor), (2, 5));
}

#[test]
fn parse_v10_20() {
    let v = ProtocolVersion::parse("abp/v10.20").unwrap();
    assert_eq!((v.major, v.minor), (10, 20));
}

#[test]
fn parse_v0_0() {
    let v = ProtocolVersion::parse("abp/v0.0").unwrap();
    assert_eq!((v.major, v.minor), (0, 0));
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Version ordering
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn ordering_v0_1_lt_v0_2() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 0, minor: 2 };
    assert!(a < b);
}

#[test]
fn ordering_v0_2_lt_v1_0() {
    let a = ProtocolVersion { major: 0, minor: 2 };
    let b = ProtocolVersion { major: 1, minor: 0 };
    assert!(a < b);
}

#[test]
fn ordering_v1_0_lt_v2_0() {
    let a = ProtocolVersion { major: 1, minor: 0 };
    let b = ProtocolVersion { major: 2, minor: 0 };
    assert!(a < b);
}

#[test]
fn ordering_chain() {
    let versions: Vec<ProtocolVersion> = [(0, 1), (0, 2), (1, 0), (2, 0)]
        .iter()
        .map(|&(maj, min)| ProtocolVersion {
            major: maj,
            minor: min,
        })
        .collect();
    for w in versions.windows(2) {
        assert!(w[0] < w[1], "{} should be < {}", w[0], w[1]);
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Exact match negotiation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_exact_match_v0_1() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    assert_eq!(negotiate_version(&v, &v).unwrap(), v);
}

#[test]
fn negotiate_exact_match_v1_5() {
    let v = ProtocolVersion { major: 1, minor: 5 };
    assert_eq!(negotiate_version(&v, &v).unwrap(), v);
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Minor forward compatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_minor_forward_compat_client_v0_1_server_v0_2() {
    let client = ProtocolVersion { major: 0, minor: 1 };
    let server = ProtocolVersion { major: 0, minor: 2 };
    let result = negotiate_version(&client, &server).unwrap();
    assert_eq!(result, client, "should pick the lower minor");
}

#[test]
fn negotiate_minor_forward_compat_within_major_1() {
    let client = ProtocolVersion { major: 1, minor: 0 };
    let server = ProtocolVersion { major: 1, minor: 9 };
    let result = negotiate_version(&client, &server).unwrap();
    assert_eq!(result, client);
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Major incompatibility
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_major_incompatible_v1_vs_v2() {
    let client = ProtocolVersion { major: 1, minor: 0 };
    let server = ProtocolVersion { major: 2, minor: 0 };
    let err = negotiate_version(&client, &server).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

#[test]
fn negotiate_major_incompatible_v0_vs_v1() {
    let client = ProtocolVersion { major: 0, minor: 9 };
    let server = ProtocolVersion { major: 1, minor: 0 };
    let err = negotiate_version(&client, &server).unwrap_err();
    match &err {
        VersionError::Incompatible { local, remote } => {
            assert_eq!(local, &client);
            assert_eq!(remote, &server);
        }
        _ => panic!("expected Incompatible, got {err:?}"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 6. Range negotiation — contains both endpoints
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn range_contains_min_and_max() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert!(range.contains(&range.min));
    assert!(range.contains(&range.max));
}

#[test]
fn range_contains_midpoint() {
    let range = VersionRange {
        min: ProtocolVersion { major: 1, minor: 0 },
        max: ProtocolVersion {
            major: 1,
            minor: 10,
        },
    };
    assert!(range.contains(&ProtocolVersion { major: 1, minor: 5 }));
}

// ═══════════════════════════════════════════════════════════════════════════
// 7. Range edge cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn range_single_version() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let range = VersionRange {
        min: v.clone(),
        max: v.clone(),
    };
    assert!(range.contains(&v));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 2 }));
}

#[test]
fn range_empty_min_gt_max() {
    // When min > max, nothing should be contained.
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 5 },
        max: ProtocolVersion { major: 0, minor: 1 },
    };
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 3 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 1 }));
    assert!(!range.contains(&ProtocolVersion { major: 0, minor: 5 }));
}

#[test]
fn range_compatible_rejects_other_major() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 3 },
    };
    assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
}

#[test]
fn overlapping_ranges_share_versions() {
    let r1 = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 4 },
    };
    let r2 = VersionRange {
        min: ProtocolVersion { major: 0, minor: 3 },
        max: ProtocolVersion { major: 0, minor: 6 },
    };
    // v0.3 and v0.4 are in both ranges
    let shared = ProtocolVersion { major: 0, minor: 3 };
    assert!(r1.contains(&shared) && r2.contains(&shared));
    let shared2 = ProtocolVersion { major: 0, minor: 4 };
    assert!(r1.contains(&shared2) && r2.contains(&shared2));
}

// ═══════════════════════════════════════════════════════════════════════════
// 8. Multi-version negotiation (simulated via ranges)
// ═══════════════════════════════════════════════════════════════════════════

/// Simulate multi-version negotiation: given two lists of supported versions,
/// find the highest version present in both.
fn negotiate_multi(
    client: &[ProtocolVersion],
    server: &[ProtocolVersion],
) -> Option<ProtocolVersion> {
    let mut common: Vec<_> = client
        .iter()
        .filter(|v| server.contains(v))
        .cloned()
        .collect();
    common.sort();
    common.last().cloned()
}

#[test]
fn multi_version_overlap_picks_highest_common() {
    let client = vec![
        ProtocolVersion { major: 0, minor: 1 },
        ProtocolVersion { major: 0, minor: 2 },
    ];
    let server = vec![
        ProtocolVersion { major: 0, minor: 2 },
        ProtocolVersion { major: 0, minor: 3 },
    ];
    assert_eq!(
        negotiate_multi(&client, &server),
        Some(ProtocolVersion { major: 0, minor: 2 })
    );
}

#[test]
fn multi_version_no_overlap() {
    let client = vec![ProtocolVersion { major: 0, minor: 1 }];
    let server = vec![ProtocolVersion { major: 0, minor: 2 }];
    assert_eq!(negotiate_multi(&client, &server), None);
}

// ═══════════════════════════════════════════════════════════════════════════
// 9. Downgrade negotiation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_downgrade_client_v0_3_server_v0_1() {
    let client = ProtocolVersion { major: 0, minor: 3 };
    let server = ProtocolVersion { major: 0, minor: 1 };
    let result = negotiate_version(&client, &server).unwrap();
    assert_eq!(result, server, "negotiate picks min → v0.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 10. Upgrade negotiation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_upgrade_client_v0_1_server_v0_3() {
    let client = ProtocolVersion { major: 0, minor: 1 };
    let server = ProtocolVersion { major: 0, minor: 3 };
    let result = negotiate_version(&client, &server).unwrap();
    assert_eq!(result, client, "negotiate picks min → v0.1");
}

// ═══════════════════════════════════════════════════════════════════════════
// 11. Invalid version strings
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_invalid_vx_y() {
    assert!(matches!(
        ProtocolVersion::parse("abp/vX.Y").unwrap_err(),
        VersionError::InvalidMajor
    ));
}

#[test]
fn parse_invalid_missing_prefix() {
    assert!(matches!(
        ProtocolVersion::parse("0.1").unwrap_err(),
        VersionError::InvalidFormat
    ));
}

#[test]
fn parse_invalid_negative() {
    assert!(matches!(
        ProtocolVersion::parse("abp/v-1.0").unwrap_err(),
        VersionError::InvalidMajor
    ));
}

#[test]
fn parse_invalid_empty() {
    assert!(matches!(
        ProtocolVersion::parse("").unwrap_err(),
        VersionError::InvalidFormat
    ));
}

#[test]
fn parse_invalid_no_dot() {
    assert!(matches!(
        ProtocolVersion::parse("abp/v1").unwrap_err(),
        VersionError::InvalidFormat
    ));
}

// ═══════════════════════════════════════════════════════════════════════════
// 12. Display roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn display_roundtrip_is_identity() {
    for (maj, min) in [(0, 0), (0, 1), (1, 0), (2, 5), (10, 20)] {
        let v = ProtocolVersion {
            major: maj,
            minor: min,
        };
        let displayed = v.to_string();
        let reparsed = ProtocolVersion::parse(&displayed).unwrap();
        assert_eq!(v, reparsed, "roundtrip failed for {displayed}");
        assert_eq!(reparsed.to_string(), displayed, "second display differs");
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 13. Hash and Eq — usable as BTreeMap key
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn version_as_btreemap_key() {
    let mut map = BTreeMap::new();
    let v01 = ProtocolVersion { major: 0, minor: 1 };
    let v02 = ProtocolVersion { major: 0, minor: 2 };
    map.insert(v01.clone(), "first");
    map.insert(v02.clone(), "second");
    assert_eq!(map[&v01], "first");
    assert_eq!(map[&v02], "second");
    assert_eq!(map.len(), 2);
}

#[test]
fn version_as_btreemap_key_dedup() {
    let mut map = BTreeMap::new();
    let v = ProtocolVersion { major: 1, minor: 0 };
    map.insert(v.clone(), "a");
    map.insert(v.clone(), "b");
    assert_eq!(map.len(), 1);
    assert_eq!(map[&v], "b");
}

#[test]
fn version_hash_consistency() {
    use std::collections::HashMap;
    let v = ProtocolVersion { major: 3, minor: 7 };
    let mut hmap = HashMap::new();
    hmap.insert(v.clone(), 42);
    assert_eq!(hmap[&v], 42);
}

// ═══════════════════════════════════════════════════════════════════════════
// 14. Serde roundtrip
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn serde_roundtrip_protocol_version() {
    let v = ProtocolVersion { major: 2, minor: 7 };
    let json = serde_json::to_string(&v).unwrap();
    let back: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, back);
}

#[test]
fn serde_roundtrip_version_range() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 9 },
    };
    let json = serde_json::to_string(&range).unwrap();
    let back: VersionRange = serde_json::from_str(&json).unwrap();
    assert_eq!(range, back);
}

#[test]
fn serde_json_shape() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["major"], 0);
    assert_eq!(json["minor"], 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 15. Current contract version parses successfully
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn contract_version_parses() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert_eq!(v, ProtocolVersion::current());
}

#[test]
fn contract_version_current_is_v0_1() {
    let v = ProtocolVersion::current();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

// ═══════════════════════════════════════════════════════════════════════════
// 16. Version in envelope survives serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn envelope_hello_preserves_contract_version() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test-sidecar".into(),
            backend_version: Some("1.0.0".into()),
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );

    let encoded = JsonlCodec::encode(&hello).unwrap();
    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();

    match decoded {
        Envelope::Hello {
            contract_version, ..
        } => {
            assert_eq!(contract_version, CONTRACT_VERSION);
            ProtocolVersion::parse(&contract_version)
                .expect("contract_version in hello must parse");
        }
        other => panic!("expected Hello, got {other:?}"),
    }
}

#[test]
fn envelope_hello_version_field_in_json() {
    let hello = Envelope::hello(
        BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        CapabilityManifest::new(),
    );
    let json = serde_json::to_value(&hello).unwrap();
    assert_eq!(json["contract_version"], CONTRACT_VERSION);
}

// ═══════════════════════════════════════════════════════════════════════════
// 17. Negotiation symmetry
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiation_is_symmetric_same_major() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 0, minor: 3 };
    assert_eq!(
        negotiate_version(&a, &b).unwrap(),
        negotiate_version(&b, &a).unwrap(),
    );
}

#[test]
fn negotiation_symmetric_across_many_pairs() {
    let versions: Vec<ProtocolVersion> = (0..5)
        .map(|m| ProtocolVersion { major: 0, minor: m })
        .collect();
    for a in &versions {
        for b in &versions {
            let ab = negotiate_version(a, b);
            let ba = negotiate_version(b, a);
            assert_eq!(ab.is_ok(), ba.is_ok());
            if let (Ok(ab_v), Ok(ba_v)) = (ab, ba) {
                assert_eq!(ab_v, ba_v, "symmetry violated for {a} vs {b}");
            }
        }
    }
}

#[test]
fn negotiation_symmetric_incompatible() {
    let a = ProtocolVersion { major: 0, minor: 1 };
    let b = ProtocolVersion { major: 1, minor: 0 };
    // Both should fail
    assert!(negotiate_version(&a, &b).is_err());
    assert!(negotiate_version(&b, &a).is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 18. Reflexive negotiation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn negotiate_reflexive() {
    for (maj, min) in [(0, 0), (0, 1), (1, 0), (5, 9)] {
        let v = ProtocolVersion {
            major: maj,
            minor: min,
        };
        assert_eq!(
            negotiate_version(&v, &v).unwrap(),
            v,
            "reflexive negotiation failed for {v}"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 19. Version with whitespace
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_leading_whitespace_rejected() {
    assert!(ProtocolVersion::parse(" abp/v0.1").is_err());
}

#[test]
fn parse_trailing_whitespace_rejected() {
    assert!(ProtocolVersion::parse("abp/v0.1 ").is_err());
}

#[test]
fn parse_surrounding_whitespace_rejected() {
    assert!(ProtocolVersion::parse(" abp/v0.1 ").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 20. Clone and Copy semantics
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn version_clone_is_equal() {
    let v = ProtocolVersion { major: 4, minor: 2 };
    let cloned = v.clone();
    assert_eq!(v, cloned);
}

#[test]
fn version_clone_is_independent() {
    let v = ProtocolVersion { major: 0, minor: 1 };
    let mut cloned = v.clone();
    cloned.major = 99;
    assert_ne!(v, cloned, "clone must be independent");
    assert_eq!(v.major, 0);
}

#[test]
fn version_range_clone_is_equal() {
    let range = VersionRange {
        min: ProtocolVersion { major: 0, minor: 1 },
        max: ProtocolVersion { major: 0, minor: 5 },
    };
    assert_eq!(range, range.clone());
}

// ═══════════════════════════════════════════════════════════════════════════
// Bonus: error display messages
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn error_display_incompatible() {
    let err = VersionError::Incompatible {
        local: ProtocolVersion { major: 0, minor: 1 },
        remote: ProtocolVersion { major: 1, minor: 0 },
    };
    let msg = err.to_string();
    assert!(msg.contains("incompatible"), "got: {msg}");
    assert!(msg.contains("abp/v0.1"), "got: {msg}");
    assert!(msg.contains("abp/v1.0"), "got: {msg}");
}

#[test]
fn error_display_invalid_format() {
    let msg = VersionError::InvalidFormat.to_string();
    assert!(msg.contains("invalid version format"), "got: {msg}");
}
