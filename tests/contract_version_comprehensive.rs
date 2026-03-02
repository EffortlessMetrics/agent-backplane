// SPDX-License-Identifier: MIT OR Apache-2.0
//! Comprehensive tests for contract versioning and compatibility.

use abp_core::{
    BackendIdentity, CapabilityManifest, ExecutionMode, Outcome, ReceiptBuilder, RunMetadata,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::version::{negotiate_version, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{is_compatible_version, parse_version, Envelope, JsonlCodec};
use chrono::Utc;
use uuid::Uuid;

// =========================================================================
// Helpers
// =========================================================================

fn make_backend(id: &str) -> BackendIdentity {
    BackendIdentity {
        id: id.into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn make_hello() -> Envelope {
    Envelope::hello(make_backend("test-sidecar"), CapabilityManifest::new())
}

fn make_hello_with_mode(mode: ExecutionMode) -> Envelope {
    Envelope::hello_with_mode(
        make_backend("test-sidecar"),
        CapabilityManifest::new(),
        mode,
    )
}

// =========================================================================
// 1. CONTRACT_VERSION value verification
// =========================================================================

#[test]
fn contract_version_equals_expected() {
    assert_eq!(CONTRACT_VERSION, "abp/v0.1");
}

#[test]
fn contract_version_is_static_str() {
    let v: &'static str = CONTRACT_VERSION;
    assert!(!v.is_empty());
}

#[test]
fn contract_version_starts_with_abp_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/"));
}

#[test]
fn contract_version_has_v_prefix() {
    assert!(CONTRACT_VERSION.starts_with("abp/v"));
}

#[test]
fn contract_version_contains_dot_separator() {
    let rest = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    assert!(rest.contains('.'));
}

#[test]
fn contract_version_major_is_zero() {
    let (major, _) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(major, 0);
}

#[test]
fn contract_version_minor_is_one() {
    let (_, minor) = parse_version(CONTRACT_VERSION).unwrap();
    assert_eq!(minor, 1);
}

// =========================================================================
// 2. CONTRACT_VERSION format validation
// =========================================================================

#[test]
fn contract_version_format_no_trailing_whitespace() {
    assert_eq!(CONTRACT_VERSION, CONTRACT_VERSION.trim());
}

#[test]
fn contract_version_format_no_leading_whitespace() {
    assert!(!CONTRACT_VERSION.starts_with(' '));
}

#[test]
fn contract_version_format_ascii_only() {
    assert!(CONTRACT_VERSION.is_ascii());
}

#[test]
fn contract_version_format_no_newlines() {
    assert!(!CONTRACT_VERSION.contains('\n'));
    assert!(!CONTRACT_VERSION.contains('\r'));
}

#[test]
fn contract_version_format_no_null_bytes() {
    assert!(!CONTRACT_VERSION.contains('\0'));
}

#[test]
fn contract_version_format_exactly_two_parts_after_prefix() {
    let rest = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    let parts: Vec<&str> = rest.split('.').collect();
    assert_eq!(parts.len(), 2, "expected MAJOR.MINOR, got {rest}");
}

#[test]
fn contract_version_format_major_is_numeric() {
    let rest = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    let major = rest.split('.').next().unwrap();
    assert!(major.parse::<u32>().is_ok());
}

#[test]
fn contract_version_format_minor_is_numeric() {
    let rest = CONTRACT_VERSION.strip_prefix("abp/v").unwrap();
    let minor = rest.split('.').nth(1).unwrap();
    assert!(minor.parse::<u32>().is_ok());
}

#[test]
fn contract_version_parseable_by_parse_version() {
    assert!(parse_version(CONTRACT_VERSION).is_some());
}

#[test]
fn contract_version_parseable_by_protocol_version() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION);
    assert!(v.is_ok());
}

// =========================================================================
// 3. Version in hello envelope
// =========================================================================

#[test]
fn hello_envelope_contains_contract_version() {
    let env = make_hello();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_envelope_with_mode_contains_contract_version() {
    let env = make_hello_with_mode(ExecutionMode::Passthrough);
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_envelope_mapped_mode_has_contract_version() {
    let env = make_hello_with_mode(ExecutionMode::Mapped);
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_envelope_serialized_contains_version_field() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains("\"contract_version\""));
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn hello_envelope_round_trip_preserves_version() {
    let env = make_hello();
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = &decoded
    {
        assert_eq!(contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Hello envelope after round-trip");
    }
}

#[test]
fn hello_envelope_json_tag_is_hello() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    assert!(json.contains("\"t\":\"hello\""));
}

#[test]
fn hello_decode_custom_version_accepted() {
    let json = r#"{"t":"hello","contract_version":"abp/v99.0","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, "abp/v99.0");
    } else {
        panic!("expected Hello envelope");
    }
}

#[test]
fn hello_decode_empty_version_string_accepted() {
    let json = r#"{"t":"hello","contract_version":"","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{},"mode":"mapped"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, "");
    } else {
        panic!("expected Hello envelope");
    }
}

// =========================================================================
// 4. Version in receipt
// =========================================================================

#[test]
fn receipt_builder_sets_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_with_hash_preserves_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_serialized_contains_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&receipt).unwrap();
    assert!(json.contains(CONTRACT_VERSION));
}

#[test]
fn receipt_deserialized_preserves_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let json = serde_json::to_string(&receipt).unwrap();
    let deserialized: abp_core::Receipt = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_meta_contract_version_field_exists() {
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: Utc::now(),
        finished_at: Utc::now(),
        duration_ms: 0,
    };
    assert_eq!(meta.contract_version, CONTRACT_VERSION);
}

#[test]
fn receipt_meta_with_custom_version_roundtrips() {
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: "abp/v2.0".to_string(),
        started_at: Utc::now(),
        finished_at: Utc::now(),
        duration_ms: 0,
    };
    let json = serde_json::to_string(&meta).unwrap();
    let deserialized: RunMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.contract_version, "abp/v2.0");
}

#[test]
fn receipt_hash_deterministic_with_same_version() {
    let r1 = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build()
        .with_hash()
        .unwrap();
    // same builder — version should be the same
    assert_eq!(r1.meta.contract_version, CONTRACT_VERSION);
    assert!(r1.receipt_sha256.is_some());
}

// =========================================================================
// 5. Version in work order
// =========================================================================

#[test]
fn work_order_builder_produces_valid_work_order() {
    let wo = WorkOrderBuilder::new("test task").build();
    assert_eq!(wo.task, "test task");
}

#[test]
fn work_order_serialized_round_trips() {
    let wo = WorkOrderBuilder::new("test task").build();
    let json = serde_json::to_string(&wo).unwrap();
    let deserialized: abp_core::WorkOrder = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.task, wo.task);
    assert_eq!(deserialized.id, wo.id);
}

#[test]
fn work_order_in_run_envelope_serializes() {
    let wo = WorkOrderBuilder::new("test task").build();
    let env = Envelope::Run {
        id: wo.id.to_string(),
        work_order: wo,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("\"t\":\"run\""));
}

// =========================================================================
// 6. Version compatibility checking
// =========================================================================

#[test]
fn is_compatible_same_version() {
    assert!(is_compatible_version(CONTRACT_VERSION, CONTRACT_VERSION));
}

#[test]
fn is_compatible_same_major_different_minor() {
    assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
}

#[test]
fn is_compatible_different_major() {
    assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
}

#[test]
fn is_compatible_both_invalid_returns_false() {
    assert!(!is_compatible_version("invalid", "also-invalid"));
}

#[test]
fn is_compatible_one_invalid_returns_false() {
    assert!(!is_compatible_version(CONTRACT_VERSION, "invalid"));
}

#[test]
fn is_compatible_other_invalid_returns_false() {
    assert!(!is_compatible_version("invalid", CONTRACT_VERSION));
}

#[test]
fn is_compatible_high_minor_versions() {
    assert!(is_compatible_version("abp/v0.100", "abp/v0.200"));
}

#[test]
fn is_compatible_zero_minor() {
    assert!(is_compatible_version("abp/v0.0", "abp/v0.1"));
}

#[test]
fn negotiate_version_same() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    let result = negotiate_version(&v, &v).unwrap();
    assert_eq!(result.major, v.major);
    assert_eq!(result.minor, v.minor);
}

#[test]
fn negotiate_version_picks_minimum_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v01, &v02).unwrap();
    assert_eq!(result.minor, 1);
}

#[test]
fn negotiate_version_picks_minimum_minor_reversed() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let result = negotiate_version(&v02, &v01).unwrap();
    assert_eq!(result.minor, 1);
}

#[test]
fn negotiate_version_different_major_fails() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = negotiate_version(&v0, &v1).unwrap_err();
    assert!(matches!(err, VersionError::Incompatible { .. }));
}

#[test]
fn negotiate_version_high_majors_compatible() {
    let v5a = ProtocolVersion::parse("abp/v5.3").unwrap();
    let v5b = ProtocolVersion::parse("abp/v5.9").unwrap();
    let result = negotiate_version(&v5a, &v5b).unwrap();
    assert_eq!(result.major, 5);
    assert_eq!(result.minor, 3);
}

#[test]
fn protocol_version_is_compatible_same() {
    let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert!(v.is_compatible(&v));
}

#[test]
fn protocol_version_is_compatible_newer_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert!(v01.is_compatible(&v02));
}

#[test]
fn protocol_version_is_not_compatible_older_minor() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    // v02 requires minor >= 2; v01.minor is 1
    assert!(!v02.is_compatible(&v01));
}

#[test]
fn protocol_version_is_not_compatible_different_major() {
    let v0 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v1 = ProtocolVersion::parse("abp/v1.1").unwrap();
    assert!(!v0.is_compatible(&v1));
}

// =========================================================================
// 7. Version string parsing
// =========================================================================

#[test]
fn parse_version_valid() {
    assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
}

#[test]
fn parse_version_zero_zero() {
    assert_eq!(parse_version("abp/v0.0"), Some((0, 0)));
}

#[test]
fn parse_version_high_numbers() {
    assert_eq!(parse_version("abp/v999.888"), Some((999, 888)));
}

#[test]
fn parse_version_single_digit() {
    assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
}

#[test]
fn parse_version_missing_prefix() {
    assert_eq!(parse_version("v0.1"), None);
}

#[test]
fn parse_version_wrong_prefix() {
    assert_eq!(parse_version("xyz/v0.1"), None);
}

#[test]
fn parse_version_missing_v() {
    assert_eq!(parse_version("abp/0.1"), None);
}

#[test]
fn parse_version_missing_dot() {
    assert_eq!(parse_version("abp/v01"), None);
}

#[test]
fn parse_version_empty_string() {
    assert_eq!(parse_version(""), None);
}

#[test]
fn parse_version_just_prefix() {
    assert_eq!(parse_version("abp/v"), None);
}

#[test]
fn parse_version_negative_major() {
    assert_eq!(parse_version("abp/v-1.0"), None);
}

#[test]
fn parse_version_alpha_major() {
    assert_eq!(parse_version("abp/va.0"), None);
}

#[test]
fn parse_version_alpha_minor() {
    assert_eq!(parse_version("abp/v0.b"), None);
}

#[test]
fn parse_version_trailing_text() {
    assert_eq!(parse_version("abp/v0.1.2"), None);
}

#[test]
fn parse_version_whitespace() {
    assert_eq!(parse_version(" abp/v0.1"), None);
}

#[test]
fn parse_version_trailing_whitespace() {
    assert_eq!(parse_version("abp/v0.1 "), None);
}

#[test]
fn protocol_version_parse_valid() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(v.major, 0);
    assert_eq!(v.minor, 1);
}

#[test]
fn protocol_version_parse_invalid_format() {
    let err = ProtocolVersion::parse("invalid").unwrap_err();
    assert_eq!(err, VersionError::InvalidFormat);
}

#[test]
fn protocol_version_parse_no_dot() {
    let err = ProtocolVersion::parse("abp/v1").unwrap_err();
    assert_eq!(err, VersionError::InvalidFormat);
}

#[test]
fn protocol_version_parse_alpha_major() {
    let err = ProtocolVersion::parse("abp/vX.1").unwrap_err();
    assert_eq!(err, VersionError::InvalidMajor);
}

#[test]
fn protocol_version_parse_alpha_minor() {
    let err = ProtocolVersion::parse("abp/v0.Y").unwrap_err();
    assert_eq!(err, VersionError::InvalidMinor);
}

#[test]
fn protocol_version_to_string() {
    let v = ProtocolVersion::parse("abp/v3.7").unwrap();
    assert_eq!(v.to_string(), "abp/v3.7");
}

#[test]
fn protocol_version_display() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(format!("{v}"), "abp/v0.1");
}

#[test]
fn protocol_version_current_matches_contract() {
    let current = ProtocolVersion::current();
    assert_eq!(current.to_string(), CONTRACT_VERSION);
}

#[test]
fn protocol_version_current_major_zero() {
    let current = ProtocolVersion::current();
    assert_eq!(current.major, 0);
}

#[test]
fn protocol_version_current_minor_one() {
    let current = ProtocolVersion::current();
    assert_eq!(current.minor, 1);
}

#[test]
fn protocol_version_round_trip_to_string_parse() {
    let v = ProtocolVersion { major: 4, minor: 5 };
    let s = v.to_string();
    let v2 = ProtocolVersion::parse(&s).unwrap();
    assert_eq!(v, v2);
}

#[test]
fn protocol_version_ordering() {
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v02 = ProtocolVersion::parse("abp/v0.2").unwrap();
    let v10 = ProtocolVersion::parse("abp/v1.0").unwrap();
    assert!(v01 < v02);
    assert!(v02 < v10);
}

#[test]
fn protocol_version_eq() {
    let a = ProtocolVersion::parse("abp/v0.1").unwrap();
    let b = ProtocolVersion::parse("abp/v0.1").unwrap();
    assert_eq!(a, b);
}

#[test]
fn protocol_version_ne() {
    let a = ProtocolVersion::parse("abp/v0.1").unwrap();
    let b = ProtocolVersion::parse("abp/v0.2").unwrap();
    assert_ne!(a, b);
}

// =========================================================================
// 8. All crates reference same version constant
// =========================================================================

#[test]
fn protocol_hello_uses_core_contract_version() {
    let env = Envelope::hello(make_backend("sidecar"), CapabilityManifest::new());
    if let Envelope::Hello {
        contract_version, ..
    } = &env
    {
        assert_eq!(contract_version, abp_core::CONTRACT_VERSION);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn receipt_builder_uses_core_contract_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    assert_eq!(receipt.meta.contract_version, abp_core::CONTRACT_VERSION);
}

#[test]
fn protocol_version_current_uses_core_contract_version() {
    let current = ProtocolVersion::current();
    let expected = ProtocolVersion::parse(abp_core::CONTRACT_VERSION).unwrap();
    assert_eq!(current, expected);
}

#[test]
fn parse_version_agrees_with_protocol_version_parse() {
    let (major, minor) = parse_version(CONTRACT_VERSION).unwrap();
    let pv = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
    assert_eq!(major, pv.major);
    assert_eq!(minor, pv.minor);
}

// =========================================================================
// 9. Schema / serialization backward compatibility
// =========================================================================

#[test]
fn hello_json_schema_has_expected_keys() {
    let json = JsonlCodec::encode(&make_hello()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(json.trim()).unwrap();
    let obj = parsed.as_object().unwrap();
    assert!(obj.contains_key("t"));
    assert!(obj.contains_key("contract_version"));
    assert!(obj.contains_key("backend"));
    assert!(obj.contains_key("capabilities"));
}

#[test]
fn hello_with_mode_field_defaults_to_mapped() {
    // When mode is absent from JSON, it should default to Mapped
    let json = r#"{"t":"hello","contract_version":"abp/v0.1","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
    let env = JsonlCodec::decode(json).unwrap();
    if let Envelope::Hello { mode, .. } = &env {
        assert_eq!(*mode, ExecutionMode::Mapped);
    } else {
        panic!("expected Hello");
    }
}

#[test]
fn receipt_serialization_includes_contract_version_path() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let val: serde_json::Value = serde_json::to_value(&receipt).unwrap();
    let cv = val
        .get("meta")
        .and_then(|m| m.get("contract_version"))
        .and_then(|v| v.as_str());
    assert_eq!(cv, Some(CONTRACT_VERSION));
}

#[test]
fn run_metadata_json_contains_contract_version() {
    let meta = RunMetadata {
        run_id: Uuid::nil(),
        work_order_id: Uuid::nil(),
        contract_version: CONTRACT_VERSION.to_string(),
        started_at: Utc::now(),
        finished_at: Utc::now(),
        duration_ms: 0,
    };
    let json = serde_json::to_string(&meta).unwrap();
    assert!(json.contains("\"contract_version\":\"abp/v0.1\""));
}

#[test]
fn final_envelope_round_trips_with_receipt_version() {
    let receipt = ReceiptBuilder::new("mock")
        .outcome(Outcome::Complete)
        .build();
    let env = Envelope::Final {
        ref_id: "run-1".into(),
        receipt,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    if let Envelope::Final { receipt, .. } = decoded {
        assert_eq!(receipt.meta.contract_version, CONTRACT_VERSION);
    } else {
        panic!("expected Final");
    }
}

// =========================================================================
// 10. Edge cases: future versions, malformed versions
// =========================================================================

#[test]
fn parse_version_future_major() {
    assert_eq!(parse_version("abp/v100.0"), Some((100, 0)));
}

#[test]
fn parse_version_future_minor() {
    assert_eq!(parse_version("abp/v0.999"), Some((0, 999)));
}

#[test]
fn protocol_version_parse_max_u32() {
    let v = ProtocolVersion::parse("abp/v4294967295.4294967295").unwrap();
    assert_eq!(v.major, u32::MAX);
    assert_eq!(v.minor, u32::MAX);
}

#[test]
fn protocol_version_parse_overflow_major() {
    let err = ProtocolVersion::parse("abp/v4294967296.0").unwrap_err();
    assert_eq!(err, VersionError::InvalidMajor);
}

#[test]
fn protocol_version_parse_overflow_minor() {
    let err = ProtocolVersion::parse("abp/v0.4294967296").unwrap_err();
    assert_eq!(err, VersionError::InvalidMinor);
}

#[test]
fn parse_version_double_dot() {
    assert_eq!(parse_version("abp/v0..1"), None);
}

#[test]
fn parse_version_leading_zero_major() {
    // "00" parses as 0 via u32::parse — this is fine
    assert_eq!(parse_version("abp/v00.1"), Some((0, 1)));
}

#[test]
fn parse_version_leading_zero_minor() {
    assert_eq!(parse_version("abp/v0.01"), Some((0, 1)));
}

#[test]
fn parse_version_plus_sign() {
    // "+1" parses as u32 in Rust, so this is a valid version
    assert_eq!(parse_version("abp/v+1.0"), Some((1, 0)));
}

#[test]
fn parse_version_float_minor() {
    assert_eq!(parse_version("abp/v0.1.0"), None);
}

#[test]
fn is_compatible_empty_strings() {
    assert!(!is_compatible_version("", ""));
}

#[test]
fn is_compatible_empty_vs_valid() {
    assert!(!is_compatible_version("", CONTRACT_VERSION));
}

#[test]
fn is_compatible_valid_vs_empty() {
    assert!(!is_compatible_version(CONTRACT_VERSION, ""));
}

#[test]
fn negotiate_version_zero_zero_compatible() {
    let v00 = ProtocolVersion::parse("abp/v0.0").unwrap();
    let v01 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let result = negotiate_version(&v00, &v01).unwrap();
    assert_eq!(result.minor, 0);
}

#[test]
fn version_range_contains_lower_bound() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.1").unwrap()));
}

#[test]
fn version_range_contains_upper_bound() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.3").unwrap()));
}

#[test]
fn version_range_contains_middle() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(range.contains(&ProtocolVersion::parse("abp/v0.2").unwrap()));
}

#[test]
fn version_range_excludes_below() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.0").unwrap()));
}

#[test]
fn version_range_excludes_above() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.3").unwrap(),
    };
    assert!(!range.contains(&ProtocolVersion::parse("abp/v0.4").unwrap()));
}

#[test]
fn version_range_is_compatible_same_major() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    assert!(range.is_compatible(&ProtocolVersion::parse("abp/v0.3").unwrap()));
}

#[test]
fn version_range_not_compatible_different_major() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    assert!(!range.is_compatible(&ProtocolVersion::parse("abp/v1.3").unwrap()));
}

#[test]
fn version_range_single_version() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let range = VersionRange {
        min: v.clone(),
        max: v.clone(),
    };
    assert!(range.contains(&v));
}

#[test]
fn version_error_incompatible_display() {
    let local = ProtocolVersion::parse("abp/v0.1").unwrap();
    let remote = ProtocolVersion::parse("abp/v1.0").unwrap();
    let err = VersionError::Incompatible {
        local: local.clone(),
        remote: remote.clone(),
    };
    let msg = err.to_string();
    assert!(msg.contains("incompatible"));
    assert!(msg.contains("abp/v0.1"));
    assert!(msg.contains("abp/v1.0"));
}

#[test]
fn version_error_invalid_format_display() {
    let err = VersionError::InvalidFormat;
    assert!(err.to_string().contains("invalid version format"));
}

#[test]
fn version_error_invalid_major_display() {
    let err = VersionError::InvalidMajor;
    assert!(err.to_string().contains("major"));
}

#[test]
fn version_error_invalid_minor_display() {
    let err = VersionError::InvalidMinor;
    assert!(err.to_string().contains("minor"));
}

#[test]
fn protocol_version_serde_round_trip() {
    let v = ProtocolVersion::parse("abp/v2.5").unwrap();
    let json = serde_json::to_string(&v).unwrap();
    let deserialized: ProtocolVersion = serde_json::from_str(&json).unwrap();
    assert_eq!(v, deserialized);
}

#[test]
fn version_range_serde_round_trip() {
    let range = VersionRange {
        min: ProtocolVersion::parse("abp/v0.1").unwrap(),
        max: ProtocolVersion::parse("abp/v0.5").unwrap(),
    };
    let json = serde_json::to_string(&range).unwrap();
    let deserialized: VersionRange = serde_json::from_str(&json).unwrap();
    assert_eq!(range, deserialized);
}

#[test]
fn protocol_version_hash_consistent() {
    use std::collections::HashSet;
    let v1 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let v2 = ProtocolVersion::parse("abp/v0.1").unwrap();
    let mut set = HashSet::new();
    set.insert(v1);
    set.insert(v2);
    assert_eq!(set.len(), 1);
}

#[test]
fn protocol_version_clone() {
    let v = ProtocolVersion::parse("abp/v0.1").unwrap();
    let cloned = v.clone();
    assert_eq!(v, cloned);
}

#[test]
fn is_compatible_symmetric_for_same_major() {
    // Compatibility should be symmetric (both share same major → both return true)
    assert!(is_compatible_version("abp/v0.3", "abp/v0.7"));
    assert!(is_compatible_version("abp/v0.7", "abp/v0.3"));
}

#[test]
fn negotiate_version_commutative_on_result() {
    let a = ProtocolVersion::parse("abp/v0.3").unwrap();
    let b = ProtocolVersion::parse("abp/v0.7").unwrap();
    let r1 = negotiate_version(&a, &b).unwrap();
    let r2 = negotiate_version(&b, &a).unwrap();
    assert_eq!(r1, r2);
}

#[test]
fn parse_version_special_characters() {
    assert_eq!(parse_version("abp/v0.1!"), None);
}

#[test]
fn parse_version_unicode() {
    assert_eq!(parse_version("abp/v0.①"), None);
}

#[test]
fn parse_version_semver_three_parts() {
    // SemVer format should fail (only major.minor supported)
    assert_eq!(parse_version("abp/v0.1.0"), None);
}

#[test]
fn parse_version_hex_numbers() {
    // "0x1" does not parse as u32 via parse()
    assert_eq!(parse_version("abp/v0x1.0"), None);
}
