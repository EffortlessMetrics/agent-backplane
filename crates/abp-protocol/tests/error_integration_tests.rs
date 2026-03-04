#![allow(clippy::all)]
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
/// Tests for abp-error integration with the protocol layer.
use abp_error::{AbpError, ErrorCode};
use abp_protocol::{Envelope, JsonlCodec, ProtocolError};

// ---------------------------------------------------------------------------
// Error code roundtrip through protocol envelopes
// ---------------------------------------------------------------------------

#[test]
fn fatal_with_error_code_roundtrips_through_jsonl() {
    let env = Envelope::fatal_with_code(
        Some("run-1".into()),
        "backend timed out",
        ErrorCode::BackendTimeout,
    );
    let json = JsonlCodec::encode(&env).unwrap();
    assert!(json.contains("backend_timeout"));

    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(decoded.error_code(), Some(ErrorCode::BackendTimeout));
    if let Envelope::Fatal {
        ref_id,
        error,
        error_code,
    } = decoded
    {
        assert_eq!(ref_id.as_deref(), Some("run-1"));
        assert_eq!(error, "backend timed out");
        assert_eq!(error_code, Some(ErrorCode::BackendTimeout));
    } else {
        panic!("expected Fatal envelope");
    }
}

#[test]
fn fatal_without_error_code_roundtrips() {
    let env = Envelope::Fatal {
        ref_id: None,
        error: "generic error".into(),
        error_code: None,
    };
    let json = JsonlCodec::encode(&env).unwrap();
    // error_code should be absent from JSON when None
    assert!(!json.contains("error_code"));

    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(decoded.error_code(), None);
}

#[test]
fn fatal_from_abp_error_preserves_code() {
    let abp_err =
        AbpError::new(ErrorCode::PolicyDenied, "not allowed").with_context("rule", "no_write");
    let env = Envelope::fatal_from_abp_error(Some("run-2".into()), &abp_err);

    let json = JsonlCodec::encode(&env).unwrap();
    let decoded = JsonlCodec::decode(json.trim()).unwrap();
    assert_eq!(decoded.error_code(), Some(ErrorCode::PolicyDenied));
}

#[test]
fn legacy_fatal_json_without_error_code_deserializes() {
    // Sidecars running older protocol versions won't send error_code.
    let json = r#"{"t":"fatal","ref_id":"run-old","error":"something broke"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert_eq!(env.error_code(), None);
    if let Envelope::Fatal {
        error_code, error, ..
    } = env
    {
        assert_eq!(error, "something broke");
        assert_eq!(error_code, None);
    } else {
        panic!("expected Fatal");
    }
}

#[test]
fn fatal_json_with_error_code_deserializes() {
    let json = r#"{"t":"fatal","ref_id":null,"error":"bad envelope","error_code":"protocol_invalid_envelope"}"#;
    let env = JsonlCodec::decode(json).unwrap();
    assert_eq!(env.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn non_fatal_envelopes_have_no_error_code() {
    let hello = Envelope::hello(
        abp_core::BackendIdentity {
            id: "test".into(),
            backend_version: None,
            adapter_version: None,
        },
        abp_core::CapabilityManifest::new(),
    );
    assert_eq!(hello.error_code(), None);
}

// ---------------------------------------------------------------------------
// ProtocolError ↔ AbpError integration
// ---------------------------------------------------------------------------

#[test]
fn abp_error_converts_to_protocol_error() {
    let abp_err = AbpError::new(ErrorCode::ProtocolVersionMismatch, "v0.1 vs v0.2");
    let proto_err: ProtocolError = abp_err.into();
    assert_eq!(
        proto_err.error_code(),
        Some(ErrorCode::ProtocolVersionMismatch)
    );
    assert!(proto_err.to_string().contains("v0.1 vs v0.2"));
}

#[test]
fn violation_has_protocol_error_code() {
    let err = ProtocolError::Violation("bad frame".into());
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolInvalidEnvelope));
}

#[test]
fn unexpected_message_has_protocol_error_code() {
    let err = ProtocolError::UnexpectedMessage {
        expected: "hello".into(),
        got: "run".into(),
    };
    assert_eq!(err.error_code(), Some(ErrorCode::ProtocolUnexpectedMessage));
}

#[test]
fn json_error_has_no_error_code() {
    let json_err: Result<serde_json::Value, _> = serde_json::from_str("not json");
    let proto_err = ProtocolError::Json(json_err.unwrap_err());
    assert_eq!(proto_err.error_code(), None);
}

// ---------------------------------------------------------------------------
// Error code preservation across encode/decode cycles
// ---------------------------------------------------------------------------

#[test]
fn all_error_codes_survive_fatal_roundtrip() {
    let codes = [
        ErrorCode::ProtocolInvalidEnvelope,
        ErrorCode::BackendTimeout,
        ErrorCode::BackendCrashed,
        ErrorCode::PolicyDenied,
        ErrorCode::WorkspaceInitFailed,
        ErrorCode::Internal,
    ];
    for code in codes {
        let env = Envelope::fatal_with_code(Some("r".into()), "test", code);
        let json = JsonlCodec::encode(&env).unwrap();
        let decoded = JsonlCodec::decode(json.trim()).unwrap();
        assert_eq!(
            decoded.error_code(),
            Some(code),
            "code {code:?} not preserved"
        );
    }
}
