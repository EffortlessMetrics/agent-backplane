// SPDX-License-Identifier: MIT OR Apache-2.0
//! Protocol version compatibility, forward/backward compat, and protocol
//! evolution tests for the ABP JSONL wire format.

use std::collections::BTreeMap;
use std::io::BufReader;

use serde_json::json;

use abp_core::{
    AgentEvent, AgentEventKind, BackendIdentity, Capability, CapabilityManifest, ExecutionMode,
    Outcome, Receipt, RunMetadata, SupportLevel, UsageNormalized, VerificationReport,
    WorkOrderBuilder, CONTRACT_VERSION,
};
use abp_protocol::codec::StreamingCodec;
use abp_protocol::stream::StreamParser;
use abp_protocol::validate::EnvelopeValidator;
use abp_protocol::version::{self, ProtocolVersion, VersionError, VersionRange};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError, is_compatible_version, parse_version};

// =========================================================================
// Helpers
// =========================================================================

fn backend() -> BackendIdentity {
    BackendIdentity {
        id: "test-sidecar".into(),
        backend_version: Some("1.0.0".into()),
        adapter_version: None,
    }
}

fn caps() -> CapabilityManifest {
    let mut m = BTreeMap::new();
    m.insert(Capability::Streaming, SupportLevel::Native);
    m
}

fn sample_receipt() -> Receipt {
    let ts = chrono::Utc::now();
    Receipt {
        meta: RunMetadata {
            run_id: uuid::Uuid::nil(),
            work_order_id: uuid::Uuid::nil(),
            contract_version: CONTRACT_VERSION.into(),
            started_at: ts,
            finished_at: ts,
            duration_ms: 0,
        },
        backend: backend(),
        capabilities: caps(),
        outcome: Outcome::Success,
        usage: UsageNormalized::default(),
        artifacts: Vec::new(),
        event_count: 0,
        receipt_sha256: None,
        verification: VerificationReport::default(),
    }
}

// =========================================================================
// 1. Version parsing — free functions
// =========================================================================

mod version_parsing {
    use super::*;

    #[test]
    fn current_version_is_valid() {
        let (major, minor) = parse_version(CONTRACT_VERSION).expect("CONTRACT_VERSION must parse");
        assert_eq!(major, 0);
        assert_eq!(minor, 1);
    }

    #[test]
    fn parses_various_valid_versions() {
        assert_eq!(parse_version("abp/v0.0"), Some((0, 0)));
        assert_eq!(parse_version("abp/v0.1"), Some((0, 1)));
        assert_eq!(parse_version("abp/v1.0"), Some((1, 0)));
        assert_eq!(parse_version("abp/v2.3"), Some((2, 3)));
        assert_eq!(parse_version("abp/v99.100"), Some((99, 100)));
    }

    #[test]
    fn rejects_invalid_formats() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("v0.1"), None);
        assert_eq!(parse_version("abp/0.1"), None);
        assert_eq!(parse_version("abp/v"), None);
        assert_eq!(parse_version("abp/v0"), None);
        assert_eq!(parse_version("abp/v0."), None);
        assert_eq!(parse_version("abp/v.1"), None);
        assert_eq!(parse_version("abp/v0.1.0"), None);
        assert_eq!(parse_version("ABP/v0.1"), None);
        assert_eq!(parse_version("abp/v-1.0"), None);
        assert_eq!(parse_version("xyz/v0.1"), None);
        assert_eq!(parse_version("abp/vX.Y"), None);
    }
}

// =========================================================================
// 2. Version compatibility — free functions
// =========================================================================

mod version_compat_free_fns {
    use super::*;

    #[test]
    fn same_version_is_compatible() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.1"));
    }

    #[test]
    fn minor_version_mismatch_compatible() {
        assert!(is_compatible_version("abp/v0.1", "abp/v0.2"));
        assert!(is_compatible_version("abp/v0.2", "abp/v0.1"));
        assert!(is_compatible_version("abp/v0.99", "abp/v0.1"));
    }

    #[test]
    fn major_version_mismatch_incompatible() {
        assert!(!is_compatible_version("abp/v1.0", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "abp/v1.0"));
        assert!(!is_compatible_version("abp/v2.0", "abp/v1.0"));
    }

    #[test]
    fn invalid_version_is_incompatible() {
        assert!(!is_compatible_version("invalid", "abp/v0.1"));
        assert!(!is_compatible_version("abp/v0.1", "invalid"));
        assert!(!is_compatible_version("garbage", "also garbage"));
    }
}

// =========================================================================
// 3. ProtocolVersion structured type
// =========================================================================

mod protocol_version_struct {
    use super::*;

    #[test]
    fn parse_current_contract_version() {
        let v = ProtocolVersion::parse(CONTRACT_VERSION).unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
    }

    #[test]
    fn current_helper() {
        let v = ProtocolVersion::current();
        assert_eq!(v, ProtocolVersion::parse(CONTRACT_VERSION).unwrap());
    }

    #[test]
    fn roundtrip_to_string() {
        let v = ProtocolVersion::parse("abp/v3.7").unwrap();
        assert_eq!(v.to_string(), "abp/v3.7");
    }

    #[test]
    fn display_impl() {
        let v = ProtocolVersion { major: 1, minor: 2 };
        assert_eq!(format!("{v}"), "abp/v1.2");
    }

    #[test]
    fn parse_errors() {
        assert_eq!(
            ProtocolVersion::parse("invalid"),
            Err(VersionError::InvalidFormat)
        );
        assert_eq!(
            ProtocolVersion::parse("abp/vX.1"),
            Err(VersionError::InvalidMajor)
        );
        assert_eq!(
            ProtocolVersion::parse("abp/v0.Y"),
            Err(VersionError::InvalidMinor)
        );
        assert_eq!(
            ProtocolVersion::parse("abp/v0"),
            Err(VersionError::InvalidFormat)
        );
    }

    #[test]
    fn is_compatible_same_major() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        // v01 is_compatible with v02 means v02.minor >= v01.minor
        assert!(v01.is_compatible(&v02));
        // v02 is_compatible with v01 means v01.minor >= v02.minor — false
        assert!(!v02.is_compatible(&v01));
    }

    #[test]
    fn is_compatible_different_major() {
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v10 = ProtocolVersion { major: 1, minor: 0 };
        assert!(!v01.is_compatible(&v10));
        assert!(!v10.is_compatible(&v01));
    }

    #[test]
    fn ordering() {
        let v00 = ProtocolVersion { major: 0, minor: 0 };
        let v01 = ProtocolVersion { major: 0, minor: 1 };
        let v02 = ProtocolVersion { major: 0, minor: 2 };
        let v10 = ProtocolVersion { major: 1, minor: 0 };
        assert!(v00 < v01);
        assert!(v01 < v02);
        assert!(v02 < v10);

        let mut versions = vec![v10.clone(), v00.clone(), v02.clone(), v01.clone()];
        versions.sort();
        assert_eq!(versions, vec![v00, v01, v02, v10]);
    }

    #[test]
    fn serde_roundtrip() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let json = serde_json::to_string(&v).unwrap();
        let back: ProtocolVersion = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

// =========================================================================
// 4. VersionRange
// =========================================================================

mod version_range_tests {
    use super::*;

    #[test]
    fn contains_inclusive_bounds() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 1 }));
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(range.contains(&ProtocolVersion { major: 0, minor: 3 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 0 }));
        assert!(!range.contains(&ProtocolVersion { major: 0, minor: 4 }));
    }

    #[test]
    fn is_compatible_checks_major() {
        let range = VersionRange {
            min: ProtocolVersion { major: 0, minor: 1 },
            max: ProtocolVersion { major: 0, minor: 3 },
        };
        assert!(range.is_compatible(&ProtocolVersion { major: 0, minor: 2 }));
        assert!(!range.is_compatible(&ProtocolVersion { major: 1, minor: 2 }));
    }
}

// =========================================================================
// 5. Version negotiation
// =========================================================================

mod negotiation {
    use super::*;

    #[test]
    fn same_version_negotiates_to_itself() {
        let v = ProtocolVersion { major: 0, minor: 1 };
        let result = version::negotiate_version(&v, &v).unwrap();
        assert_eq!(result, v);
    }

    #[test]
    fn compatible_versions_negotiate_to_min() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 0, minor: 3 };
        let result = version::negotiate_version(&local, &remote).unwrap();
        assert_eq!(result, local);

        let result2 = version::negotiate_version(&remote, &local).unwrap();
        assert_eq!(result2, local);
    }

    #[test]
    fn incompatible_major_versions_error() {
        let local = ProtocolVersion { major: 0, minor: 1 };
        let remote = ProtocolVersion { major: 1, minor: 0 };
        let err = version::negotiate_version(&local, &remote).unwrap_err();
        match err {
            VersionError::Incompatible {
                local: l,
                remote: r,
            } => {
                assert_eq!(l, local);
                assert_eq!(r, remote);
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[test]
    fn negotiation_is_commutative_for_compatible() {
        let a = ProtocolVersion { major: 0, minor: 2 };
        let b = ProtocolVersion { major: 0, minor: 5 };
        assert_eq!(
            version::negotiate_version(&a, &b).unwrap(),
            version::negotiate_version(&b, &a).unwrap()
        );
    }

    #[test]
    fn negotiation_scenario_both_sides_current() {
        let local = ProtocolVersion::current();
        let remote = ProtocolVersion::current();
        let agreed = version::negotiate_version(&local, &remote).unwrap();
        assert_eq!(agreed, ProtocolVersion::current());
    }

    #[test]
    fn negotiation_scenario_remote_newer_minor() {
        let local = ProtocolVersion::current();
        let remote = ProtocolVersion {
            major: local.major,
            minor: local.minor + 5,
        };
        let agreed = version::negotiate_version(&local, &remote).unwrap();
        assert_eq!(agreed, local);
    }
}

// =========================================================================
// 6. Hello envelope version acceptance
// =========================================================================

mod hello_envelope_version {
    use super::*;

    fn hello_json(version: &str) -> String {
        let obj = json!({
            "t": "hello",
            "contract_version": version,
            "backend": { "id": "test", "backend_version": null, "adapter_version": null },
            "capabilities": {},
            "mode": "mapped"
        });
        serde_json::to_string(&obj).unwrap()
    }

    #[test]
    fn current_version_accepted() {
        let line = hello_json(CONTRACT_VERSION);
        let env = JsonlCodec::decode(&line).unwrap();
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                assert_eq!(contract_version, CONTRACT_VERSION);
                assert!(is_compatible_version(&contract_version, CONTRACT_VERSION));
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn future_minor_version_accepted() {
        let line = hello_json("abp/v0.99");
        let env = JsonlCodec::decode(&line).unwrap();
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                assert!(is_compatible_version(&contract_version, CONTRACT_VERSION));
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn different_major_version_rejected() {
        let line = hello_json("abp/v1.0");
        let env = JsonlCodec::decode(&line).unwrap();
        match env {
            Envelope::Hello {
                contract_version, ..
            } => {
                assert!(!is_compatible_version(&contract_version, CONTRACT_VERSION));
            }
            other => panic!("expected Hello, got {other:?}"),
        }
    }

    #[test]
    fn missing_version_field_is_error() {
        let line = r#"{"t":"hello","backend":{"id":"test","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err(), "missing contract_version should error");
    }

    #[test]
    fn empty_version_string_fails_validation() {
        let line = hello_json("");
        let env = JsonlCodec::decode(&line).unwrap();
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| matches!(
                e,
                abp_protocol::validate::ValidationError::EmptyField { field }
                if field == "contract_version"
            )),
        );
    }

    #[test]
    fn invalid_version_string_fails_validation() {
        let line = hello_json("not-a-version");
        let env = JsonlCodec::decode(&line).unwrap();
        let validator = EnvelopeValidator::new();
        let result = validator.validate(&env);
        assert!(!result.valid);
        assert!(
            result.errors.iter().any(|e| matches!(
                e,
                abp_protocol::validate::ValidationError::InvalidVersion { .. }
            )),
        );
    }

    #[test]
    fn hello_factory_uses_contract_version() {
        let env = Envelope::hello(backend(), caps());
        match &env {
            Envelope::Hello {
                contract_version, ..
            } => assert_eq!(contract_version, CONTRACT_VERSION),
            _ => panic!("not hello"),
        }
    }
}

// =========================================================================
// 7. Forward-compatibility: unknown/extra fields
// =========================================================================

mod forward_compat {
    use super::*;

    #[test]
    fn extra_fields_in_hello_ignored() {
        let line = json!({
            "t": "hello",
            "contract_version": CONTRACT_VERSION,
            "backend": { "id": "test", "backend_version": null, "adapter_version": null },
            "capabilities": {},
            "mode": "mapped",
            "future_field": "some_value",
            "another_unknown": 42
        });
        let result = JsonlCodec::decode(&serde_json::to_string(&line).unwrap());
        assert!(result.is_ok(), "extra fields should be ignored: {result:?}");
    }

    #[test]
    fn extra_fields_in_fatal_ignored() {
        let line = json!({
            "t": "fatal",
            "ref_id": null,
            "error": "boom",
            "extra_field": true
        });
        let result = JsonlCodec::decode(&serde_json::to_string(&line).unwrap());
        assert!(result.is_ok(), "extra fields on fatal should be ignored");
    }

    #[test]
    fn extra_nested_fields_in_backend_ignored() {
        let line = json!({
            "t": "hello",
            "contract_version": CONTRACT_VERSION,
            "backend": {
                "id": "test",
                "backend_version": null,
                "adapter_version": null,
                "future_capability": { "nested": true }
            },
            "capabilities": {},
            "mode": "mapped"
        });
        let result = JsonlCodec::decode(&serde_json::to_string(&line).unwrap());
        assert!(
            result.is_ok(),
            "extra nested fields should be tolerated: {result:?}"
        );
    }

    #[test]
    fn unknown_envelope_type_is_error() {
        let line = r#"{"t":"ping","data":"hello"}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_err(), "unknown envelope type 't' should error");
    }

    #[test]
    fn envelope_roundtrip_preserves_known_fields() {
        let original = Envelope::hello(backend(), caps());
        let encoded = JsonlCodec::encode(&original).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        match (&original, &decoded) {
            (
                Envelope::Hello {
                    contract_version: cv1,
                    backend: b1,
                    mode: m1,
                    ..
                },
                Envelope::Hello {
                    contract_version: cv2,
                    backend: b2,
                    mode: m2,
                    ..
                },
            ) => {
                assert_eq!(cv1, cv2);
                assert_eq!(b1.id, b2.id);
                assert_eq!(m1, m2);
            }
            _ => panic!("roundtrip should preserve Hello variant"),
        }
    }

    #[test]
    fn json_roundtrip_all_envelope_variants() {
        let wo = WorkOrderBuilder::new("test task").build();
        let envelopes = vec![
            Envelope::hello(backend(), caps()),
            Envelope::Run {
                id: "run-1".into(),
                work_order: wo,
            },
            Envelope::Event {
                ref_id: "run-1".into(),
                event: AgentEvent {
                    timestamp: chrono::Utc::now(),
                    event: AgentEventKind::Log {
                        message: "hello".into(),
                    },
                },
            },
            Envelope::Final {
                ref_id: "run-1".into(),
                receipt: sample_receipt(),
            },
            Envelope::Fatal {
                ref_id: Some("run-1".into()),
                error: "kaboom".into(),
                error_code: None,
            },
        ];

        for original in &envelopes {
            let encoded = JsonlCodec::encode(original).unwrap();
            let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
            // Verify discriminant survives roundtrip
            let orig_tag = format!("{original:?}").split('{').next().unwrap().to_string();
            let dec_tag = format!("{decoded:?}").split('{').next().unwrap().to_string();
            assert_eq!(orig_tag, dec_tag, "variant tag must survive roundtrip");
        }
    }
}

// =========================================================================
// 8. Old/malformed envelope handling
// =========================================================================

mod old_format {
    use super::*;

    #[test]
    fn envelope_with_type_instead_of_t_is_error() {
        // The protocol uses "t" as the tag, not "type"
        let line = r#"{"type":"hello","contract_version":"abp/v0.1","backend":{"id":"x","backend_version":null,"adapter_version":null},"capabilities":{}}"#;
        let result = JsonlCodec::decode(line);
        assert!(
            result.is_err(),
            "using 'type' instead of 't' should fail: {result:?}"
        );
    }

    #[test]
    fn not_json_is_error() {
        let result = JsonlCodec::decode("this is not JSON at all");
        assert!(matches!(result, Err(ProtocolError::Json(_))));
    }

    #[test]
    fn empty_object_is_error() {
        let result = JsonlCodec::decode("{}");
        assert!(result.is_err());
    }

    #[test]
    fn json_array_is_error() {
        let result = JsonlCodec::decode("[1,2,3]");
        assert!(result.is_err());
    }

    #[test]
    fn null_is_error() {
        let result = JsonlCodec::decode("null");
        assert!(result.is_err());
    }
}

// =========================================================================
// 9. Protocol robustness — StreamParser
// =========================================================================

mod stream_parser_robustness {
    use super::*;

    fn fatal_line(msg: &str) -> String {
        let env = Envelope::Fatal {
            ref_id: None,
            error: msg.into(),
            error_code: None,
        };
        JsonlCodec::encode(&env).unwrap()
    }

    #[test]
    fn partial_lines_buffered_until_newline() {
        let mut parser = StreamParser::new();
        let full = fatal_line("partial-test");
        let (a, b) = full.as_bytes().split_at(10);

        let results = parser.push(a);
        assert!(results.is_empty(), "partial line should not yield results");
        assert!(!parser.is_empty());

        let results = parser.push(b);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn empty_lines_between_envelopes_ignored() {
        let mut parser = StreamParser::new();
        let line = fatal_line("msg");
        let input = format!("\n\n{line}\n\n{line}");
        let results = parser.push(input.as_bytes());
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn very_long_valid_line_no_panic() {
        let long_error = "x".repeat(1_000_000);
        let env = Envelope::Fatal {
            ref_id: None,
            error: long_error,
            error_code: None,
        };
        let line = JsonlCodec::encode(&env).unwrap();
        let mut parser = StreamParser::new();
        let results = parser.push(line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn line_exceeding_max_length_is_violation() {
        let mut parser = StreamParser::with_max_line_len(64);
        let long_line = format!("{}\n", "a".repeat(100));
        let results = parser.push(long_line.as_bytes());
        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], Err(ProtocolError::Violation(msg)) if msg.contains("exceeds")));
    }

    #[test]
    fn binary_data_produces_error_not_panic() {
        let mut parser = StreamParser::new();
        let binary: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x80, b'\n'];
        let results = parser.push(&binary);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err(), "binary data should produce error");
    }

    #[test]
    fn mixed_binary_and_valid_lines() {
        let mut parser = StreamParser::new();
        let valid = fatal_line("valid-msg");
        let binary_line = b"\xFF\xFE\x00\n";
        let mut input = Vec::new();
        input.extend_from_slice(binary_line);
        input.extend_from_slice(valid.as_bytes());

        let results = parser.push(&input);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_err(), "binary line should error");
        assert!(results[1].is_ok(), "valid line should parse");
    }

    #[test]
    fn multiple_envelopes_in_single_chunk() {
        let mut parser = StreamParser::new();
        let l1 = fatal_line("one");
        let l2 = fatal_line("two");
        let l3 = fatal_line("three");
        let combined = format!("{l1}{l2}{l3}");
        let results = parser.push(combined.as_bytes());
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn finish_flushes_unterminated_line() {
        let mut parser = StreamParser::new();
        let line = fatal_line("unterminated");
        let trimmed = line.trim_end().as_bytes();
        let results = parser.push(trimmed);
        assert!(results.is_empty());
        let results = parser.finish();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }

    #[test]
    fn reset_discards_buffered_data() {
        let mut parser = StreamParser::new();
        parser.push(b"partial data without newline");
        assert!(!parser.is_empty());
        parser.reset();
        assert!(parser.is_empty());
    }
}

// =========================================================================
// 10. decode_stream robustness (BufRead based)
// =========================================================================

mod decode_stream_robustness {
    use super::*;

    #[test]
    fn empty_lines_skipped() {
        let input = "\n\n\n";
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        assert!(results.is_empty());
    }

    #[test]
    fn mixed_valid_and_invalid_lines() {
        let valid = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
        let invalid = "not json";
        let input = format!("{valid}\n{invalid}\n{valid}\n");
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert!(results[1].is_err());
        assert!(results[2].is_ok());
    }

    #[test]
    fn whitespace_only_lines_ignored() {
        let valid = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
        let input = format!("  \n\t\n{valid}\n   \n");
        let reader = BufReader::new(input.as_bytes());
        let results: Vec<_> = JsonlCodec::decode_stream(reader).collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_ok());
    }
}

// =========================================================================
// 11. StreamingCodec batch operations
// =========================================================================

mod streaming_codec_tests {
    use super::*;

    #[test]
    fn encode_decode_batch_roundtrip() {
        let envelopes = vec![
            Envelope::Fatal {
                ref_id: None,
                error: "e1".into(),
                error_code: None,
            },
            Envelope::Fatal {
                ref_id: None,
                error: "e2".into(),
                error_code: None,
            },
        ];
        let batch = StreamingCodec::encode_batch(&envelopes);
        let decoded = StreamingCodec::decode_batch(&batch);
        assert_eq!(decoded.len(), 2);
        assert!(decoded.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn validate_jsonl_catches_errors() {
        let good = r#"{"t":"fatal","ref_id":null,"error":"ok"}"#;
        let bad = "not json";
        let input = format!("{good}\n{bad}\n{good}\n");
        let errors = StreamingCodec::validate_jsonl(&input);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].0, 2); // line 2 (1-based)
    }
}

// =========================================================================
// 12. Concurrent / multi-threaded envelope encoding
// =========================================================================

mod concurrent_encode_decode {
    use super::*;

    #[test]
    fn concurrent_encoding_is_safe() {
        let handles: Vec<_> = (0..10)
            .map(|i| {
                std::thread::spawn(move || {
                    let env = Envelope::Fatal {
                        ref_id: Some(format!("run-{i}")),
                        error: format!("error-{i}"),
                        error_code: None,
                    };
                    let encoded = JsonlCodec::encode(&env).unwrap();
                    let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
                    match decoded {
                        Envelope::Fatal { ref_id, error, .. } => {
                            assert_eq!(ref_id, Some(format!("run-{i}")));
                            assert_eq!(error, format!("error-{i}"));
                        }
                        _ => panic!("wrong variant"),
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }

    #[test]
    fn concurrent_stream_parser_instances() {
        let handles: Vec<_> = (0..10)
            .map(|i| {
                std::thread::spawn(move || {
                    let mut parser = StreamParser::new();
                    let env = Envelope::Fatal {
                        ref_id: None,
                        error: format!("err-{i}"),
                        error_code: None,
                    };
                    let line = JsonlCodec::encode(&env).unwrap();
                    let results = parser.push(line.as_bytes());
                    assert_eq!(results.len(), 1);
                    assert!(results[0].is_ok());
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread panicked");
        }
    }
}

// =========================================================================
// 13. Serde edge cases for protocol evolution
// =========================================================================

mod serde_compat {
    use super::*;

    #[test]
    fn missing_optional_mode_defaults() {
        // mode defaults to Mapped when absent
        let line = json!({
            "t": "hello",
            "contract_version": CONTRACT_VERSION,
            "backend": { "id": "test", "backend_version": null, "adapter_version": null },
            "capabilities": {}
        });
        let env = JsonlCodec::decode(&serde_json::to_string(&line).unwrap()).unwrap();
        match env {
            Envelope::Hello { mode, .. } => {
                assert_eq!(mode, ExecutionMode::Mapped);
            }
            _ => panic!("expected Hello"),
        }
    }

    #[test]
    fn error_code_field_absent_is_none() {
        let line = r#"{"t":"fatal","ref_id":null,"error":"oops"}"#;
        let env = JsonlCodec::decode(line).unwrap();
        assert_eq!(env.error_code(), None);
    }

    #[test]
    fn envelope_tag_is_t_not_type() {
        let env = Envelope::hello(backend(), caps());
        let json = serde_json::to_string(&env).unwrap();
        assert!(json.contains(r#""t":"hello""#), "tag should be 't': {json}");
        assert!(!json.contains(r#""type""#), "should not use 'type': {json}");
    }

    #[test]
    fn field_order_does_not_matter() {
        // JSON fields in different order than struct definition
        let line = r#"{"capabilities":{},"backend":{"id":"test","backend_version":null,"adapter_version":null},"t":"hello","contract_version":"abp/v0.1","mode":"mapped"}"#;
        let result = JsonlCodec::decode(line);
        assert!(result.is_ok(), "field order should not matter: {result:?}");
    }

    #[test]
    fn unknown_event_kind_is_error() {
        // An event envelope with an unknown event type should fail deserialization
        let line = json!({
            "t": "event",
            "ref_id": "run-1",
            "event": {
                "timestamp": "2025-01-01T00:00:00Z",
                "event": {
                    "type": "future_event_type",
                    "data": "something"
                }
            }
        });
        let result = JsonlCodec::decode(&serde_json::to_string(&line).unwrap());
        // Unknown event types should cause a parse error since AgentEventKind
        // is a tagged enum
        assert!(
            result.is_err(),
            "unknown event kind should cause parse error"
        );
    }

    #[test]
    fn fatal_envelope_with_error_code_roundtrips() {
        let env = Envelope::fatal_with_code(
            Some("run-1".into()),
            "timeout",
            abp_error::ErrorCode::ProtocolTimeout,
        );
        let encoded = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(encoded.trim()).unwrap();
        assert_eq!(
            decoded.error_code(),
            Some(abp_error::ErrorCode::ProtocolTimeout)
        );
    }
}

// =========================================================================
// 14. Encode-to-writer
// =========================================================================

mod writer_tests {
    use super::*;

    #[test]
    fn encode_to_writer_appends_newline() {
        let mut buf = Vec::new();
        let env = Envelope::Fatal {
            ref_id: None,
            error: "test".into(),
            error_code: None,
        };
        JsonlCodec::encode_to_writer(&mut buf, &env).unwrap();
        assert!(buf.ends_with(b"\n"));
    }

    #[test]
    fn encode_many_produces_valid_jsonl() {
        let envs = vec![
            Envelope::Fatal {
                ref_id: None,
                error: "a".into(),
                error_code: None,
            },
            Envelope::Fatal {
                ref_id: None,
                error: "b".into(),
                error_code: None,
            },
        ];
        let mut buf = Vec::new();
        JsonlCodec::encode_many_to_writer(&mut buf, &envs).unwrap();
        let output = String::from_utf8(buf).unwrap();
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert!(JsonlCodec::decode(line).is_ok());
        }
    }
}

// =========================================================================
// 15. Validator sequence tests related to protocol version
// =========================================================================

mod validator_sequence {
    use super::*;

    #[test]
    fn valid_sequence_has_no_errors() {
        let wo = WorkOrderBuilder::new("test").build();
        let seq = vec![
            Envelope::hello(backend(), caps()),
            Envelope::Run {
                id: "run-1".into(),
                work_order: wo,
            },
            Envelope::Event {
                ref_id: "run-1".into(),
                event: AgentEvent {
                    timestamp: chrono::Utc::now(),
                    event: AgentEventKind::Log {
                        message: "hi".into(),
                    },
                },
            },
            Envelope::Final {
                ref_id: "run-1".into(),
                receipt: sample_receipt(),
            },
        ];
        let v = EnvelopeValidator::new();
        let errors = v.validate_sequence(&seq);
        assert!(errors.is_empty(), "valid sequence should have no errors: {errors:?}");
    }

    #[test]
    fn missing_hello_detected() {
        let wo = WorkOrderBuilder::new("test").build();
        let seq = vec![
            Envelope::Run {
                id: "run-1".into(),
                work_order: wo,
            },
            Envelope::Final {
                ref_id: "run-1".into(),
                receipt: sample_receipt(),
            },
        ];
        let v = EnvelopeValidator::new();
        let errors = v.validate_sequence(&seq);
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, abp_protocol::validate::SequenceError::MissingHello)),
        );
    }
}

// =========================================================================
// 16. ProtocolError taxonomy
// =========================================================================

mod protocol_error_taxonomy {
    use super::*;

    #[test]
    fn violation_has_error_code() {
        let err = ProtocolError::Violation("test".into());
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolInvalidEnvelope)
        );
    }

    #[test]
    fn unexpected_message_has_error_code() {
        let err = ProtocolError::UnexpectedMessage {
            expected: "hello".into(),
            got: "run".into(),
        };
        assert_eq!(
            err.error_code(),
            Some(abp_error::ErrorCode::ProtocolUnexpectedMessage)
        );
    }

    #[test]
    fn json_error_has_no_code() {
        let err: ProtocolError = serde_json::from_str::<Envelope>("bad").unwrap_err().into();
        assert_eq!(err.error_code(), None);
    }
}
