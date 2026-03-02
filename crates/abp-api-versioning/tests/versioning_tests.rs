// SPDX-License-Identifier: MIT OR Apache-2.0
//! Tests for the API versioning module.

use abp_api_versioning::*;

// ---------------------------------------------------------------------------
// ApiVersion::parse
// ---------------------------------------------------------------------------

#[test]
fn parse_major_only() {
    let v = ApiVersion::parse("v1").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 0);
}

#[test]
fn parse_major_and_minor() {
    let v = ApiVersion::parse("v2.3").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 3);
}

#[test]
fn parse_without_v_prefix() {
    let v = ApiVersion::parse("1.2").unwrap();
    assert_eq!(v, ApiVersion { major: 1, minor: 2 });
}

#[test]
fn parse_zero_version() {
    let v = ApiVersion::parse("v0").unwrap();
    assert_eq!(v, ApiVersion { major: 0, minor: 0 });
}

#[test]
fn parse_empty_string_fails() {
    assert!(matches!(
        ApiVersion::parse(""),
        Err(ApiVersionError::InvalidFormat(_))
    ));
}

#[test]
fn parse_bare_v_fails() {
    assert!(matches!(
        ApiVersion::parse("v"),
        Err(ApiVersionError::InvalidFormat(_))
    ));
}

#[test]
fn parse_invalid_major_fails() {
    assert!(matches!(
        ApiVersion::parse("vX.1"),
        Err(ApiVersionError::InvalidFormat(_))
    ));
}

#[test]
fn parse_invalid_minor_fails() {
    assert!(matches!(
        ApiVersion::parse("v1.abc"),
        Err(ApiVersionError::InvalidFormat(_))
    ));
}

// ---------------------------------------------------------------------------
// ApiVersion::is_compatible
// ---------------------------------------------------------------------------

#[test]
fn compatible_same_major() {
    let a = ApiVersion { major: 1, minor: 0 };
    let b = ApiVersion { major: 1, minor: 5 };
    assert!(a.is_compatible(&b));
}

#[test]
fn incompatible_different_major() {
    let a = ApiVersion { major: 1, minor: 0 };
    let b = ApiVersion { major: 2, minor: 0 };
    assert!(!a.is_compatible(&b));
}

// ---------------------------------------------------------------------------
// Display / Ord
// ---------------------------------------------------------------------------

#[test]
fn display_format() {
    let v = ApiVersion { major: 3, minor: 7 };
    assert_eq!(v.to_string(), "v3.7");
}

#[test]
fn ordering_major_takes_precedence() {
    let a = ApiVersion { major: 1, minor: 9 };
    let b = ApiVersion { major: 2, minor: 0 };
    assert!(a < b);
}

#[test]
fn ordering_minor_within_same_major() {
    let a = ApiVersion { major: 1, minor: 0 };
    let b = ApiVersion { major: 1, minor: 1 };
    assert!(a < b);
}

#[test]
fn ordering_equal() {
    let a = ApiVersion { major: 1, minor: 0 };
    let b = ApiVersion { major: 1, minor: 0 };
    assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal);
}

// ---------------------------------------------------------------------------
// ApiVersionError Display
// ---------------------------------------------------------------------------

#[test]
fn error_invalid_format_display() {
    let e = ApiVersionError::InvalidFormat("oops".into());
    assert!(e.to_string().contains("oops"));
}

#[test]
fn error_unsupported_version_display() {
    let v = ApiVersion { major: 9, minor: 0 };
    let e = ApiVersionError::UnsupportedVersion(v);
    assert!(e.to_string().contains("v9.0"));
}

#[test]
fn error_implements_std_error() {
    let e: Box<dyn std::error::Error> = Box::new(ApiVersionError::InvalidFormat("test".into()));
    assert!(!e.to_string().is_empty());
}

// ---------------------------------------------------------------------------
// VersionedEndpoint + ApiVersionRegistry
// ---------------------------------------------------------------------------

fn sample_registry() -> ApiVersionRegistry {
    let mut reg = ApiVersionRegistry::new(ApiVersion { major: 2, minor: 0 });

    reg.register(VersionedEndpoint {
        path: "/health".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });

    reg.register(VersionedEndpoint {
        path: "/legacy".into(),
        min_version: ApiVersion { major: 1, minor: 0 },
        max_version: Some(ApiVersion { major: 1, minor: 9 }),
        deprecated: true,
        deprecated_message: Some("use /v2/new instead".into()),
    });

    reg.register(VersionedEndpoint {
        path: "/new".into(),
        min_version: ApiVersion { major: 2, minor: 0 },
        max_version: None,
        deprecated: false,
        deprecated_message: None,
    });

    reg
}

#[test]
fn registry_current_version() {
    let reg = sample_registry();
    assert_eq!(*reg.current_version(), ApiVersion { major: 2, minor: 0 });
}

#[test]
fn registry_is_supported_present() {
    let reg = sample_registry();
    assert!(reg.is_supported("/health", &ApiVersion { major: 1, minor: 0 }));
}

#[test]
fn registry_is_supported_unbounded_max() {
    let reg = sample_registry();
    assert!(reg.is_supported(
        "/health",
        &ApiVersion {
            major: 99,
            minor: 0
        }
    ));
}

#[test]
fn registry_is_supported_respects_max() {
    let reg = sample_registry();
    assert!(reg.is_supported("/legacy", &ApiVersion { major: 1, minor: 5 }));
    assert!(!reg.is_supported("/legacy", &ApiVersion { major: 2, minor: 0 }));
}

#[test]
fn registry_is_supported_unknown_path() {
    let reg = sample_registry();
    assert!(!reg.is_supported("/nope", &ApiVersion { major: 1, minor: 0 }));
}

#[test]
fn registry_deprecated_endpoints() {
    let reg = sample_registry();
    let deprecated = reg.deprecated_endpoints();
    assert_eq!(deprecated.len(), 1);
    assert_eq!(deprecated[0].path, "/legacy");
    assert_eq!(
        deprecated[0].deprecated_message.as_deref(),
        Some("use /v2/new instead")
    );
}

#[test]
fn registry_supported_versions() {
    let reg = sample_registry();
    let versions = reg.supported_versions();
    assert!(versions.contains(&ApiVersion { major: 1, minor: 0 }));
    assert!(versions.contains(&ApiVersion { major: 2, minor: 0 }));
}

#[test]
fn registry_endpoints_for_version() {
    let reg = sample_registry();
    let v1 = reg.endpoints_for_version(&ApiVersion { major: 1, minor: 0 });
    let paths: Vec<&str> = v1.iter().map(|ep| ep.path.as_str()).collect();
    assert!(paths.contains(&"/health"));
    assert!(paths.contains(&"/legacy"));
    assert!(!paths.contains(&"/new"));

    let v2 = reg.endpoints_for_version(&ApiVersion { major: 2, minor: 0 });
    let paths2: Vec<&str> = v2.iter().map(|ep| ep.path.as_str()).collect();
    assert!(paths2.contains(&"/health"));
    assert!(paths2.contains(&"/new"));
    assert!(!paths2.contains(&"/legacy"));
}

// ---------------------------------------------------------------------------
// VersionNegotiator
// ---------------------------------------------------------------------------

#[test]
fn negotiate_exact_match() {
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 2, minor: 0 },
    ];
    let req = ApiVersion { major: 1, minor: 0 };
    assert_eq!(VersionNegotiator::negotiate(&req, &supported), Some(req));
}

#[test]
fn negotiate_picks_highest_compatible() {
    let supported = vec![
        ApiVersion { major: 1, minor: 0 },
        ApiVersion { major: 1, minor: 2 },
        ApiVersion { major: 1, minor: 5 },
        ApiVersion { major: 2, minor: 0 },
    ];
    let req = ApiVersion { major: 1, minor: 3 };
    assert_eq!(
        VersionNegotiator::negotiate(&req, &supported),
        Some(ApiVersion { major: 1, minor: 2 })
    );
}

#[test]
fn negotiate_no_compatible_version() {
    let supported = vec![ApiVersion { major: 2, minor: 0 }];
    let req = ApiVersion { major: 1, minor: 0 };
    assert_eq!(VersionNegotiator::negotiate(&req, &supported), None);
}

#[test]
fn negotiate_empty_supported() {
    let req = ApiVersion { major: 1, minor: 0 };
    assert_eq!(VersionNegotiator::negotiate(&req, &[]), None);
}
